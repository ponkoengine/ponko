//! Declared replication from C (Rust `Replication`): the game gives the
//! engine its message endpoint (`ponko_game_messages`), declares which of
//! its components replicate and how each field travels
//! (`ponko_ctx_replication`, in setup), and marks entities
//! (`ponko_ctx_replicate`). The engine keeps the clients' worlds in step
//! with the server's: delta compression, interest, the bandwidth budget,
//! spawns and despawns. Inputs, prediction, events and interpolation are
//! in `replication_raw`.

use std::ffi::c_char;

use ponko::net::{
    InterestConfig, InterestTier, MAX_INTEREST_TIERS, NetId, PeerId, Quantize, RawField, RawType, Replica, Replicated,
    Replication, ReplicationRole,
};
use ponko::prelude::{Entity, Vec3};

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::game::build;
use crate::messages::PonkoNetMessages;
use crate::replicate::PonkoInterestConfig;
use crate::three::PonkoVec3;
use crate::{Failure, PonkoGame, PonkoStatus, boundary, error, invalid, invalid_owned};

/// The type of a replicated field (Rust `RawType`).
/// ```
/// assert_eq!(ponko_ffi::PonkoReplicatedType::PONKO_REPLICATED_BOOL as u32, 7);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoReplicatedType {
    /// `float`.
    PONKO_REPLICATED_F32,
    /// `int32_t`.
    PONKO_REPLICATED_I32,
    /// `uint32_t`.
    PONKO_REPLICATED_U32,
    /// `int16_t`.
    PONKO_REPLICATED_I16,
    /// `uint16_t`.
    PONKO_REPLICATED_U16,
    /// `int8_t`.
    PONKO_REPLICATED_I8,
    /// `uint8_t`.
    PONKO_REPLICATED_U8,
    /// `bool` (one byte).
    PONKO_REPLICATED_BOOL,
}

/// How a field is rounded for the wire (Rust `Quantize`).
/// ```
/// assert_eq!(ponko_ffi::PonkoFieldQuantize::PONKO_FIELD_BITS as u32, 3);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoFieldQuantize {
    /// The value as it is, at its type's full width.
    PONKO_FIELD_EXACT,
    /// A float between `min` and `max` in steps of `step` (clamped).
    PONKO_FIELD_RANGE,
    /// An angle in radians, in `bits` bits (2 to 32).
    PONKO_FIELD_ANGLE,
    /// The low `bits` bits of an integer.
    PONKO_FIELD_BITS,
}

/// One replicated field of a component: where it is, its type and how it
/// travels. `min`, `max` and `step` are read for `PONKO_FIELD_RANGE`,
/// `bits` for `_ANGLE` and `_BITS`.
/// ```
/// let f = ponko_ffi::PonkoReplicatedField {
///     name: c"x".as_ptr(), offset: 0,
///     ty: ponko_ffi::PonkoReplicatedType::PONKO_REPLICATED_F32,
///     quantize: ponko_ffi::PonkoFieldQuantize::PONKO_FIELD_RANGE,
///     min: -100.0, max: 100.0, step: 0.01, bits: 0,
/// };
/// assert_eq!(f.offset, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoReplicatedField {
    /// Its name: server and clients must declare the same names.
    pub name: *const c_char,
    /// Its byte offset in the component (`offsetof`).
    pub offset: usize,
    /// Its type.
    pub ty: PonkoReplicatedType,
    /// How it is rounded.
    pub quantize: PonkoFieldQuantize,
    /// `PONKO_FIELD_RANGE`: the smallest value sent.
    pub min: f32,
    /// `PONKO_FIELD_RANGE`: the largest value sent.
    pub max: f32,
    /// `PONKO_FIELD_RANGE`: the precision.
    pub step: f32,
    /// `PONKO_FIELD_ANGLE` and `_BITS`: bits per value.
    pub bits: u8,
}

/// A component to replicate: its id from `ponko_component_register` (or a
/// built-in such as `ponko_transform_component`) and its fields. Fields
/// not listed don't travel: a client's copy keeps zero there.
/// ```
/// let c = ponko_ffi::PonkoReplicatedComponent { name: c"hp".as_ptr(), component: 0, fields: std::ptr::null(), field_count: 0 };
/// assert_eq!(c.field_count, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoReplicatedComponent {
    /// Its name on the wire: server and clients must use the same.
    pub name: *const c_char,
    /// Its component id in this world.
    pub component: u32,
    /// `field_count` fields.
    pub fields: *const PonkoReplicatedField,
    /// How many fields.
    pub field_count: usize,
}

/// How an entity replicates (Rust `Replicated`): its owning peer, if any
/// (`has_owner`, `owner`), how much it matters when the budget is tight
/// (`priority`, 1 by default) and whether every client gets it regardless
/// of interest (`global`).
/// ```
/// let r = ponko_ffi::ponko_replicated_default();
/// assert!(!r.has_owner && r.priority == 1.0 && !r.global);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoReplicated {
    /// Whether a peer owns it.
    pub has_owner: bool,
    /// The owning peer (0 is the server's own player).
    pub owner: u64,
    /// Relative importance; at least 0.
    pub priority: f32,
    /// Sent to every client regardless of interest.
    pub global: bool,
}

/// Defaults: no owner, priority 1, not global.
/// ```
/// assert_eq!(ponko_ffi::ponko_replicated_default().priority, 1.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_replicated_default() -> PonkoReplicated {
    PonkoReplicated {
        has_owner: false,
        owner: 0,
        priority: 1.0,
        global: false,
    }
}

/// Which side this game is (Rust `ReplicationRole`).
/// ```
/// assert_eq!(ponko_ffi::PonkoReplicationRole::PONKO_REPLICATION_CLIENT as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoReplicationRole {
    /// Nothing declared, or before the first tick.
    PONKO_REPLICATION_OFF,
    /// Hosting, or no endpoint: the world is the truth.
    PONKO_REPLICATION_SERVER,
    /// Joined a server: the world mirrors it.
    PONKO_REPLICATION_CLIENT,
}

/// Where replication is (`ponko_ctx_replication_status`).
/// ```
/// let s = ponko_ffi::PonkoReplicationStatus::default();
/// assert!(!s.joined);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoReplicationStatus {
    /// This side.
    pub role: PonkoReplicationRole,
    /// Server: always; client: the server accepted its declarations.
    pub joined: bool,
    /// Whether `me` is known.
    pub has_me: bool,
    /// This side's peer id (0 on the server).
    pub me: u64,
    /// The newest server tick (the server's own tick on the server).
    pub server_tick: u64,
    /// Client: the server refused it (different declarations).
    pub refused: bool,
}

impl Default for PonkoReplicationStatus {
    fn default() -> Self {
        Self {
            role: PonkoReplicationRole::PONKO_REPLICATION_OFF,
            joined: false,
            has_me: false,
            me: 0,
            server_tick: 0,
            refused: false,
        }
    }
}

/// A replica on a client (Rust `Replica`).
/// ```
/// let r = ponko_ffi::PonkoReplica::default();
/// assert!(!r.mine);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoReplica {
    /// Its net id, the same on every side.
    pub id: u32,
    /// Whether a peer owns it.
    pub has_owner: bool,
    /// The owning peer.
    pub owner: u64,
    /// Bumped on every change of owner.
    pub epoch: u32,
    /// Owned by this client.
    pub mine: bool,
}

/// What the server sent one client (Rust `PeerStats`).
/// ```
/// assert_eq!(ponko_ffi::PonkoReplicationPeerStats::default().bytes_sent, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct PonkoReplicationPeerStats {
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub packets_acked: u64,
    pub packets_lost: u64,
    pub records_sent: u64,
    pub creates_sent: u64,
    pub fulls_sent: u64,
    pub deltas_sent: u64,
    pub destroys_sent: u64,
    pub deferred: u64,
}

/// How the server resends to one client (Rust `ResendStats`, from
/// `ponko_ctx_replication_resend_stats`): records sent again early (the
/// acknowledgements skipped their packet) and on the timeout, packets
/// called lost that were acknowledged after all, the resend timeout now in
/// ticks, the timeouts in a row that doubled it, and the reordering
/// tolerated in ticks. A struct of its own, so that
/// `PonkoReplicationPeerStats` keeps its layout.
/// ```
/// assert_eq!(ponko_ffi::PonkoReplicationResendStats::default().early_resends, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct PonkoReplicationResendStats {
    pub early_resends: u64,
    pub timeout_resends: u64,
    pub late_acks: u64,
    pub timeout_ticks: u32,
    pub backoff: u32,
    pub reorder_ticks: u32,
}

/// What a client received and sent (Rust `ClientStats`).
/// ```
/// assert_eq!(ponko_ffi::PonkoReplicationClientStats::default().spawned, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct PonkoReplicationClientStats {
    pub packets_received: u64,
    pub bytes_received: u64,
    pub packets_stale: u64,
    pub packets_bad: u64,
    pub records_applied: u64,
    pub spawned: u64,
    pub despawned: u64,
    pub packets_sent: u64,
    pub bytes_sent: u64,
}

/// Gives the game a message endpoint (from `ponko_net_messages_host`,
/// `_join` or `_loopback`): the engine polls it every frame, and
/// replication runs over it. Takes the endpoint: don't use or free it
/// after this call, even when it fails.
///
/// Ownership: `endpoint` is taken over (also on failure): the game frees it;
/// don't use or free it after.
///
/// # Safety
/// `game` null or live; `endpoint` null or a live endpoint, not used after.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_messages(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_messages(game: *mut PonkoGame, endpoint: *mut PonkoNetMessages) -> PonkoStatus {
    if endpoint.is_null() {
        return boundary(|| Err(invalid("endpoint is null")));
    }
    // SAFETY: the caller hands over a live endpoint from `Box::into_raw`.
    let endpoint = unsafe { Box::from_raw(endpoint) };
    // SAFETY: forwarded caller contract.
    unsafe { build(game, move |g| (g.messages(endpoint.net), Ok(()))) }
}

/// Makes a game poll this endpoint on its own clock instead of the wall
/// clock (Rust `NetMessages::game_clock`): resends and timeouts follow the
/// frames a game runs, so a headless run plays the same every time.
/// # Safety
/// `endpoint` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_game_clock(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_game_clock(endpoint: *mut PonkoNetMessages) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let m = unsafe { endpoint.as_mut() }.ok_or_else(|| invalid("endpoint is null"))?;
        let net = std::mem::replace(&mut m.net, ponko::net::NetMessages::loopback(2).remove(0));
        m.net = net.game_clock();
        Ok(())
    })
}

fn raw_type(t: PonkoReplicatedType) -> RawType {
    use PonkoReplicatedType::*;
    match t {
        PONKO_REPLICATED_F32 => RawType::F32,
        PONKO_REPLICATED_I32 => RawType::I32,
        PONKO_REPLICATED_U32 => RawType::U32,
        PONKO_REPLICATED_I16 => RawType::I16,
        PONKO_REPLICATED_U16 => RawType::U16,
        PONKO_REPLICATED_I8 => RawType::I8,
        PONKO_REPLICATED_U8 => RawType::U8,
        PONKO_REPLICATED_BOOL => RawType::Bool,
    }
}

/// The field as Rust's, checked the way the declaration would check it
/// (so a mistake is an error here, not a panic later).
pub(crate) fn field(f: &PonkoReplicatedField, component: &str, size: usize) -> Result<RawField, Failure> {
    // SAFETY: the caller guarantees NUL-terminated field names.
    let name = unsafe { text(f.name) }?;
    if name.is_empty() {
        return Err(invalid_owned(format!("a field of `{component}` has no name")));
    }
    let ty = raw_type(f.ty);
    if f.offset.checked_add(ty.size()).is_none_or(|end| end > size) {
        return Err(invalid_owned(format!(
            "field `{name}` of `{component}` lies outside its {size} bytes"
        )));
    }
    let quantize = match f.quantize {
        PonkoFieldQuantize::PONKO_FIELD_EXACT => Quantize::Exact,
        PonkoFieldQuantize::PONKO_FIELD_RANGE => Quantize::range(f.min, f.max, f.step),
        PonkoFieldQuantize::PONKO_FIELD_ANGLE => Quantize::Angle { bits: f.bits },
        PonkoFieldQuantize::PONKO_FIELD_BITS => Quantize::Bits(f.bits),
    };
    let width = (ty.size() * 8) as u32;
    let fits = match (ty, quantize) {
        (_, Quantize::Exact) => true,
        (RawType::F32, Quantize::Range { .. }) => quantize.bits_per_value().is_some(),
        (RawType::F32, Quantize::Angle { bits }) => (2..=32).contains(&bits),
        (RawType::F32 | RawType::Bool, _) => false,
        (_, Quantize::Bits(bits)) => (1..=width).contains(&u32::from(bits)),
        _ => false,
    };
    if !fits {
        return Err(invalid_owned(format!(
            "field `{name}` of `{component}`: a float takes EXACT, RANGE (finite min < max, step > 0) or ANGLE (2-32 bits); an integer EXACT or BITS (1 to its width); a bool EXACT"
        )));
    }
    Ok(RawField::new(name, f.offset, ty, quantize))
}

/// Declares what replicates, the same on the server and every client, in
/// setup (before the first tick): `count` components with their fields,
/// a bandwidth `budget` per client in bytes per second (0: 64 KB/s), and
/// optional `interest` rings (null: every client gets every entity). An
/// entity's position, for interest, is its `Transform2D` (or
/// `Transform3D`). Replaces an earlier declaration.
/// # Safety
/// Live ctx; `components` readable for `count` values, each with readable
/// fields and NUL-terminated names; `interest` null or readable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication(std::ptr::null_mut(), std::ptr::null(), 0, 0, std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication(
    ctx: *mut PonkoCtx,
    components: *const PonkoReplicatedComponent,
    count: usize,
    budget: u32,
    interest: *const PonkoInterestConfig,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let list = unsafe { slice(components, count, "components") }?;
        if list.len() > 64 {
            return Err(invalid("at most 64 component types replicate"));
        }
        // SAFETY: forwarded caller contract.
        let interest = match unsafe { interest.as_ref() } {
            None => None,
            Some(c) => {
                let n = c.tier_count as usize;
                if !(1..=MAX_INTEREST_TIERS).contains(&n) {
                    return Err(invalid("tier_count must be 1 to 8"));
                }
                let config = InterestConfig {
                    tiers: c.tiers[..n]
                        .iter()
                        .map(|t| InterestTier {
                            radius: t.radius,
                            every: t.every,
                        })
                        .collect(),
                    hysteresis: c.hysteresis,
                    stagger: c.stagger,
                };
                config.validate().map_err(invalid)?;
                Some(config)
            }
        };
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                if c.net.replication.role() != ReplicationRole::Off {
                    return Err(invalid("declare replication in setup, before the first tick"));
                }
                let mut declared: Vec<(&str, _, usize, Vec<RawField>)> = Vec::with_capacity(list.len());
                for item in list {
                    let name = text(item.name)?;
                    if name.is_empty() || declared.iter().any(|(n, ..)| *n == name) {
                        return Err(invalid_owned(format!(
                            "replicated component names must be nonempty and different (`{name}`)"
                        )));
                    }
                    let id = c
                        .world
                        .component_at(item.component as usize)
                        .ok_or_else(|| invalid("component id is not registered in this world"))?;
                    let info = c.world.component_info(id);
                    if info.needs_drop() {
                        return Err(invalid_owned(format!("`{name}` isn't plain bytes")));
                    }
                    let size = info.layout().size();
                    let fields = slice(item.fields, item.field_count, "fields")?
                        .iter()
                        .map(|f| field(f, name, size))
                        .collect::<Result<Vec<_>, _>>()?;
                    declared.push((name, id, size, fields));
                }
                let mut r = Replication::new().budget(if budget == 0 {
                    ponko::net::DEFAULT_BUDGET
                } else {
                    budget
                });
                if let Some(config) = interest {
                    r = r.interest(config);
                }
                for (name, id, size, fields) in declared {
                    r = r.raw_component(name, id, size, fields);
                }
                c.net.replication = r;
                Ok(())
            })
        }
    })
}

/// # Safety
/// `ptr` null or readable for `len` values.
pub(crate) unsafe fn slice<'a, T>(ptr: *const T, len: usize, what: &str) -> Result<&'a [T], Failure> {
    if len == 0 {
        Ok(&[])
    } else if ptr.is_null() {
        Err(invalid_owned(format!("{what} is null")))
    } else {
        // SAFETY: forwarded caller contract.
        Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

pub(crate) fn not_found(what: &'static str) -> Failure {
    error(PonkoStatus::PONKO_NOT_FOUND, what)
}

/// Server: marks `entity` as replicated (`how`: null for the defaults of
/// `ponko_replicated_default`), or changes how. Its declared components
/// reach every client it interests.
/// # Safety
/// Live ctx; `how` null or readable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replicate(std::ptr::null_mut(), 0, std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replicate(
    ctx: *mut PonkoCtx,
    entity: u64,
    how: *const PonkoReplicated,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let how = unsafe { how.as_ref() }
            .copied()
            .unwrap_or_else(|| ponko_replicated_default());
        if !(how.priority.is_finite() && how.priority >= 0.0) {
            return Err(invalid("priority must be finite and at least 0"));
        }
        let owner = how.has_owner.then(|| PeerId::from_bits(how.owner));
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                let e = Entity::from_bits(entity);
                if !c.world.is_alive(e) {
                    return Err(not_found("entity is not alive"));
                }
                if let Some(r) = c.world.get_mut::<Replicated>(e) {
                    if r.owner() != owner {
                        r.set_owner(owner);
                    }
                    r.set_priority(how.priority);
                    r.set_global(how.global);
                    return Ok(());
                }
                let mut r = match owner {
                    Some(o) => Replicated::owned_by(o),
                    None => Replicated::new(),
                }
                .with_priority(how.priority);
                if how.global {
                    r = r.global();
                }
                c.world.insert(e, r);
                Ok(())
            })
        }
    })
}

/// Where replication is: this side, whether it joined, its peer id and the
/// newest server tick.
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_status(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_status(
    ctx: *mut PonkoCtx,
    out_status: *mut PonkoReplicationStatus,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let status = ctx_access(ctx, |c| {
                let r = &c.net.replication;
                let me = r.me();
                Ok(PonkoReplicationStatus {
                    role: match r.role() {
                        ReplicationRole::Off => PonkoReplicationRole::PONKO_REPLICATION_OFF,
                        ReplicationRole::Server => PonkoReplicationRole::PONKO_REPLICATION_SERVER,
                        ReplicationRole::Client => PonkoReplicationRole::PONKO_REPLICATION_CLIENT,
                    },
                    joined: r.is_joined(),
                    has_me: me.is_some(),
                    me: me.map_or(0, PeerId::to_bits),
                    server_tick: r.server_tick(),
                    refused: r.error().is_some(),
                })
            })?;
            out(out_status, status)
        }
    })
}

/// The net id of `entity`: an original on the server once it was sent
/// (the tick after it was marked), or a replica on a client.
/// `PONKO_NOT_FOUND` if it has none.
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_net_id(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_net_id(ctx: *mut PonkoCtx, entity: u64, out_id: *mut u32) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let id = ctx_access(ctx, |c| {
                let e = Entity::from_bits(entity);
                c.world
                    .get::<Replica>(e)
                    .map(Replica::id)
                    .or_else(|| c.world.get::<Replicated>(e).and_then(Replicated::net_id))
                    .ok_or_else(|| not_found("entity has no net id"))
            })?;
            out(out_id, id.index())
        }
    })
}

/// The entity of net id `id` on this side. `PONKO_NOT_FOUND` if there is
/// none (not yet received, or gone).
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_net_entity(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_net_entity(ctx: *mut PonkoCtx, id: u32, out_entity: *mut u64) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let e = ctx_access(ctx, |c| {
                c.net
                    .replication
                    .entity(NetId::from_index(id))
                    .ok_or_else(|| not_found("no entity has this net id here"))
            })?;
            out(out_entity, e.to_bits())
        }
    })
}

/// Client: what this replica is (`PONKO_NOT_FOUND` for an entity that
/// isn't one).
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replica(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replica(
    ctx: *mut PonkoCtx,
    entity: u64,
    out_replica: *mut PonkoReplica,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let r = ctx_access(ctx, |c| {
                let r = c
                    .world
                    .get::<Replica>(Entity::from_bits(entity))
                    .ok_or_else(|| not_found("entity is not a replica"))?;
                Ok(PonkoReplica {
                    id: r.id().index(),
                    has_owner: r.owner().is_some(),
                    owner: r.owner().map_or(0, PeerId::to_bits),
                    epoch: u32::from(r.epoch()),
                    mine: r.is_mine(),
                })
            })?;
            out(out_replica, r)
        }
    })
}

/// Server: the clients that joined, in id order. Writes up to `capacity`
/// peer ids to `out_peers` and the count of all of them to `*out_count`.
/// # Safety
/// Live ctx; `out_peers` writable for `capacity` values (null if 0);
/// writable `out_count`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_clients(std::ptr::null_mut(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_clients(
    ctx: *mut PonkoCtx,
    out_peers: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_count.is_null() || (capacity > 0 && out_peers.is_null()) {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let peers = ctx_access(ctx, |c| Ok(c.net.replication.clients()))?;
            for (i, p) in peers.iter().take(capacity).enumerate() {
                out_peers.add(i).write(p.to_bits());
            }
            out(out_count, peers.len())
        }
    })
}

/// Server: where `peer` looks from, for interest (a spectator camera);
/// null goes back to its owned entities' positions.
/// # Safety
/// Live ctx; `position` null or readable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_set_viewer(std::ptr::null_mut(), 1, std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_set_viewer(
    ctx: *mut PonkoCtx,
    peer: u64,
    position: *const PonkoVec3,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let at = unsafe { position.as_ref() }.map(|p| Vec3::new(p.x, p.y, p.z));
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                c.net.replication.set_viewer(PeerId::from_bits(peer), at);
                Ok(())
            })
        }
    })
}

/// Server: what it sent `peer` (`PONKO_NOT_FOUND` if no such client).
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_peer_stats(std::ptr::null_mut(), 1, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_peer_stats(
    ctx: *mut PonkoCtx,
    peer: u64,
    out_stats: *mut PonkoReplicationPeerStats,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let s = ctx_access(ctx, |c| {
                c.net
                    .replication
                    .peer_stats(PeerId::from_bits(peer))
                    .ok_or_else(|| not_found("no such client"))
            })?;
            out(
                out_stats,
                PonkoReplicationPeerStats {
                    packets_sent: s.packets_sent,
                    bytes_sent: s.bytes_sent,
                    packets_acked: s.packets_acked,
                    packets_lost: s.packets_lost,
                    records_sent: s.records_sent,
                    creates_sent: s.creates_sent,
                    fulls_sent: s.fulls_sent,
                    deltas_sent: s.deltas_sent,
                    destroys_sent: s.destroys_sent,
                    deferred: s.deferred,
                },
            )
        }
    })
}

/// Server: how it resends to `peer` (`PONKO_NOT_FOUND` if no such client).
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_resend_stats(std::ptr::null_mut(), 1, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_resend_stats(
    ctx: *mut PonkoCtx,
    peer: u64,
    out_stats: *mut PonkoReplicationResendStats,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let s = ctx_access(ctx, |c| {
                c.net
                    .replication
                    .resend_stats(PeerId::from_bits(peer))
                    .ok_or_else(|| not_found("no such client"))
            })?;
            out(
                out_stats,
                PonkoReplicationResendStats {
                    early_resends: s.early_resends,
                    timeout_resends: s.timeout_resends,
                    late_acks: s.late_acks,
                    timeout_ticks: s.timeout_ticks,
                    backoff: s.backoff,
                    reorder_ticks: s.reorder_ticks,
                },
            )
        }
    })
}

/// Client: what it received and sent.
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_client_stats(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_client_stats(
    ctx: *mut PonkoCtx,
    out_stats: *mut PonkoReplicationClientStats,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let s = ctx_access(ctx, |c| Ok(c.net.replication.client_stats()))?;
            out(
                out_stats,
                PonkoReplicationClientStats {
                    packets_received: s.packets_received,
                    bytes_received: s.bytes_received,
                    packets_stale: s.packets_stale,
                    packets_bad: s.packets_bad,
                    records_applied: s.records_applied,
                    spawned: s.spawned,
                    despawned: s.despawned,
                    packets_sent: s.packets_sent,
                    bytes_sent: s.bytes_sent,
                },
            )
        }
    })
}
