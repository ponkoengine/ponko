//! Replication inputs, prediction, events, interpolation and the interest
//! position for games that declare their types from C (Rust
//! `Replication::raw_input`, `raw_predicted`, `raw_interpolated`,
//! `raw_event` and `position`). Declare them in setup, after
//! `ponko_ctx_replication` (which replaces every earlier declaration), the
//! same on the server and every client.
use std::ffi::{c_char, c_void};

use ponko::net::{NetEvent, PeerId, RawEvent, RawType, Replication, ReplicationRole, SnapshotConfig};
use ponko::prelude::Vec3;
use ponko_ecs::ComponentId;

use crate::component_hooks::PonkoFreeUserFn;
use crate::ctx::{PonkoCtx, ctx_access, events_kept, out, text};
use crate::replicate::PonkoSnapshotConfig;
use crate::replication::{PonkoReplicatedField, field, not_found, slice};
use crate::{Failure, PonkoStatus, boundary, invalid, invalid_owned};

/// The largest input, in bytes.
const MAX_INPUT: usize = 1024;

/// Amends the declaration in setup: refused once the game ticks, and
/// before `ponko_ctx_replication`.
unsafe fn declare<T>(
    ctx: *mut PonkoCtx,
    check: impl FnOnce(&mut ponko::prelude::Ctx, &Replication) -> Result<T, Failure>,
    then: impl FnOnce(Replication, T) -> Replication,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                let r = &c.net.replication;
                if !r.is_active() {
                    return Err(invalid("declare the components with ponko_ctx_replication first"));
                }
                if r.role() != ReplicationRole::Off {
                    return Err(invalid("declare replication in setup, before the first tick"));
                }
                let r = std::mem::take(&mut c.net.replication);
                match check(c, &r) {
                    Ok(checked) => {
                        c.net.replication = then(r, checked);
                        Ok(())
                    }
                    Err(e) => {
                        c.net.replication = r;
                        Err(e)
                    }
                }
            })
        }
    })
}

/// Declares the game's input as `size` bytes (at most 1024), of which
/// `fields` travel (Rust `Replication::raw_input`): what each player
/// controls this tick, set with `ponko_ctx_set_replication_input` and read
/// on the server with `ponko_ctx_replication_input_of`. Inputs travel every
/// tick, redundantly, so a lost packet costs nothing. A game has one input
/// type: put every control in it. Call in setup, after
/// `ponko_ctx_replication`; a second input is `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `name` and `fields` (with their names) are read during the
/// call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_input(std::ptr::null_mut(), c"Move".as_ptr(), 4, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a C string; `fields` readable for
/// `field_count` values with C-string names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_input(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    size: usize,
    fields: *const PonkoReplicatedField,
    field_count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        declare(
            ctx,
            |_, r| {
                let name = text(name)?;
                if name.is_empty() || !(1..=MAX_INPUT).contains(&size) {
                    return Err(invalid("an input has a name and 1 to 1024 bytes"));
                }
                if r.has_input() {
                    return Err(invalid("an input is declared already; a game has one input type"));
                }
                let list = slice(fields, field_count, "fields")?
                    .iter()
                    .map(|f| field(f, name, size))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((name, list))
            },
            |r, (name, list)| r.raw_input(name, size, list),
        )
    }
}

/// Steps one predicted component for one input: changes the component's
/// `state_size` bytes at `state` the way the server's game does, for the
/// input's `input_size` bytes at `input`. It runs on the server for every
/// input a player sends, and on the owning client to predict and to
/// replay after a correction, so it must give the same result for the
/// same bytes and must not call into the ctx.
/// ```
/// let step: ponko_ffi::PonkoPredictFn = None;
/// assert!(step.is_none());
/// ```
pub type PonkoPredictFn = Option<unsafe extern "C" fn(*mut c_void, usize, *const c_void, usize, *mut c_void)>;

/// A C step and its data; frees the data with the declaration.
struct CStep {
    step: unsafe extern "C" fn(*mut c_void, usize, *const c_void, usize, *mut c_void),
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
}

// SAFETY: steps run on the game's thread (the replication systems); the
// caller promises `user` may be used there and freed where the ctx is
// freed (the contract of `ponko_ctx_replication_predicted`).
unsafe impl Send for CStep {}
// SAFETY: as above; the step is only called, never changed.
unsafe impl Sync for CStep {}

impl CStep {
    fn call(&self, state: &mut [u8], input: &[u8]) {
        // SAFETY: the caller's step contract; both slices live for the call.
        unsafe {
            (self.step)(
                state.as_mut_ptr().cast(),
                state.len(),
                input.as_ptr().cast(),
                input.len(),
                self.user,
            );
        }
    }
}

impl Drop for CStep {
    fn drop(&mut self) {
        if let Some(free) = self.free_user {
            // SAFETY: the caller's contract: `free_user(user)` is called
            // exactly once, when the declaration is done with it.
            unsafe { free(self.user) };
        }
    }
}

/// Predicts the component declared as `component` with
/// `ponko_ctx_replication` on the client that owns the entity (Rust
/// `Replication::raw_predicted`): the client runs `step` for its own
/// inputs at once instead of waiting for the server, and when the
/// server's state arrives it replays the inputs the server hasn't applied
/// yet on top of it, so a correction doesn't snap back. The server runs
/// the same `step` for the inputs it receives. Declare the input
/// (`ponko_ctx_replication_input`) first. Call in setup.
///
/// Ownership: on success the declaration keeps `step` and `user` and calls
/// `free_user(user)` (if not null) once when it is dropped (another
/// `ponko_ctx_replication`, or the ctx freed); on failure the caller keeps
/// `user`. `step` runs on the game's thread.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_predicted(std::ptr::null_mut(), c"Body".as_ptr(), None, std::ptr::null_mut(), None) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `component` a C string; `step` and `free_user`
/// callable as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_predicted(
    ctx: *mut PonkoCtx,
    component: *const c_char,
    step: PonkoPredictFn,
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        declare(
            ctx,
            |_, r| {
                let component = text(component)?;
                let Some(step) = step else {
                    return Err(invalid("step is null"));
                };
                if r.raw_fields(component).is_none() {
                    return Err(invalid_owned(format!(
                        "no component `{component}` was declared with ponko_ctx_replication"
                    )));
                }
                if !r.has_raw_input() {
                    return Err(invalid("declare the input with ponko_ctx_replication_input first"));
                }
                Ok((component, step))
            },
            |r, (component, step)| {
                let c = CStep { step, user, free_user };
                r.raw_predicted(component, move |state: &mut [u8], input: &[u8]| c.call(state, input))
            },
        )
    }
}

/// Shows the component declared as `component` with `ponko_ctx_replication`
/// between the values received, on clients (Rust
/// `Replication::raw_interpolated`): its float fields move smoothly,
/// `config->delay` seconds in the past (angles, `PONKO_FIELD_ANGLE`, the
/// short way round), and its other fields are set when they arrive. At
/// most 32 float fields. `config` null: the defaults of
/// `ponko_snapshot_config_default` with no extrapolation. Call in setup.
///
/// Ownership: `component` and `config` are read during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_interpolated(std::ptr::null_mut(), c"Pos".as_ptr(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `component` a C string; `config` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_interpolated(
    ctx: *mut PonkoCtx,
    component: *const c_char,
    config: *const PonkoSnapshotConfig,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        declare(
            ctx,
            |_, r| {
                let component = text(component)?;
                let config = match config.as_ref() {
                    Some(c) => SnapshotConfig {
                        delay: c.delay,
                        max_extrapolation: c.max_extrapolation,
                        capacity: c.capacity as usize,
                    },
                    None => SnapshotConfig {
                        max_extrapolation: 0.0,
                        ..SnapshotConfig::default()
                    },
                };
                config.validate().map_err(invalid)?;
                let Some(fields) = r.raw_fields(component) else {
                    return Err(invalid_owned(format!(
                        "no component `{component}` was declared with ponko_ctx_replication"
                    )));
                };
                let floats = fields.iter().filter(|f| f.ty == RawType::F32).count();
                if floats > Replication::RAW_INTERPOLATED_FLOATS {
                    return Err(invalid_owned(format!(
                        "`{component}` has {floats} float fields; an interpolated component takes at most 32"
                    )));
                }
                Ok((component, config))
            },
            |r, (component, config)| r.raw_interpolated(component, config),
        )
    }
}

/// Declares an event kind called `name` (Rust `Replication::raw_event`):
/// bytes that travel reliably and in order, sent with
/// `ponko_ctx_replication_send` and read with `ponko_ctx_net_events`.
/// Writes its kind, the number those calls use, to `out_kind`: kinds count
/// from 0 in the order they are declared. Call in setup; a name declared
/// twice is `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `name` is read during the call only; `out_kind` is the
/// caller's.
/// ```
/// let mut kind = 0u16;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_event(std::ptr::null_mut(), c"Chat".as_ptr(), &mut kind) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a C string; `out_kind` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_event(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    out_kind: *mut u16,
) -> PonkoStatus {
    let mut declared = "";
    // SAFETY: forwarded caller contract.
    let status = unsafe {
        declare(
            ctx,
            |_, r| {
                let name = text(name)?;
                if out_kind.is_null() {
                    return Err(invalid("output is null"));
                }
                if name.is_empty() || r.raw_event_kind(name).is_some() {
                    return Err(invalid_owned(format!(
                        "event names are nonempty and declared once (`{name}`)"
                    )));
                }
                Ok(name)
            },
            |r, name| {
                declared = name;
                r.raw_event(name)
            },
        )
    };
    if status != PonkoStatus::PONKO_OK {
        return status;
    }
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                let kind = c
                    .net
                    .replication
                    .raw_event_kind(declared)
                    .ok_or_else(|| invalid("more than 65536 event kinds"))?;
                out(out_kind, kind)
            })
        }
    })
}

/// Where each replicated entity is, for interest: the 2 or 3 floats
/// (`dims`) at byte `offset` of its component `component` (an id from
/// `ponko_component_register`) instead of its transform (Rust
/// `Replication::position`). An entity without that component reaches
/// every client. Call in setup.
///
/// Ownership: nothing is kept but the numbers.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_position(std::ptr::null_mut(), 0, 0, 3) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_position(
    ctx: *mut PonkoCtx,
    component: u32,
    offset: usize,
    dims: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        declare(
            ctx,
            |c, _| {
                if !(2..=3).contains(&dims) {
                    return Err(invalid("dims is 2 or 3"));
                }
                let cid = c
                    .world
                    .component_at(component as usize)
                    .ok_or_else(|| invalid("component id is not registered in this world"))?;
                let info = c.world.component_info(cid);
                let end = offset.checked_add(dims as usize * 4);
                if info.needs_drop() || end.is_none_or(|end| end > info.layout().size()) {
                    return Err(invalid("the position must be plain floats inside the component"));
                }
                Ok(cid)
            },
            |r, id| r.position(move |world, e| position_of(world, e, id, offset, dims as usize)),
        )
    }
}

/// The `dims` floats at `offset` of component `id` of `e`, checked when
/// declared to lie inside the component.
fn position_of(
    world: &ponko::prelude::World,
    e: ponko::prelude::Entity,
    id: ComponentId,
    offset: usize,
    dims: usize,
) -> Option<Vec3> {
    let p = world.raw_ref().get_dynamic(e, id)?;
    let mut v = [0.0f32; 3];
    for (i, slot) in v.iter_mut().take(dims).enumerate() {
        // SAFETY: `ponko_ctx_replication_position` checked that `dims`
        // floats at `offset` lie inside the component's bytes.
        *slot = unsafe { p.add(offset + i * 4).cast::<f32>().read_unaligned() };
    }
    Some(Vec3::from_array(v))
}

/// Sets this side's input for this tick (Rust `Replication::set_raw_input`):
/// `size` bytes, the declared input's size (more are ignored, fewer leave
/// the rest zero). A client sends it to the server and predicts with it;
/// the server (or host) applies it as its own player's.
///
/// Ownership: `data` is copied during the call.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_replication_input(std::ptr::null_mut(), std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `data` readable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_replication_input(
    ctx: *mut PonkoCtx,
    data: *const c_void,
    size: usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                if !c.net.replication.has_raw_input() {
                    return Err(invalid("no input was declared with ponko_ctx_replication_input"));
                }
                let bytes = slice(data.cast::<u8>(), size, "data")?;
                c.net.replication.set_raw_input(bytes);
                Ok(())
            })
        }
    })
}

/// Server: copies the input of `peer` (0 for the host's own player)
/// applied last into `out_data` (Rust `Replication::raw_input_of`): up to
/// `size` bytes, the rest of `out_data` zeroed. `PONKO_NOT_FOUND` if that
/// peer sent none yet.
///
/// Ownership: `out_data` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_input_of(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_data` writable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_input_of(
    ctx: *mut PonkoCtx,
    peer: u64,
    out_data: *mut c_void,
    size: usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                if out_data.is_null() && size > 0 {
                    return Err(invalid("output is null"));
                }
                let bytes = c
                    .net
                    .replication
                    .raw_input_of(PeerId::from_bits(peer))
                    .ok_or_else(|| not_found("that peer sent no input yet"))?;
                if size > 0 {
                    let dst = std::slice::from_raw_parts_mut(out_data.cast::<u8>(), size);
                    let n = bytes.len().min(size);
                    dst[..n].copy_from_slice(&bytes[..n]);
                    dst[n..].fill(0);
                }
                Ok(())
            })
        }
    })
}

/// Where `ponko_ctx_replication_send` sends an event.
/// ```
/// assert_eq!(ponko_ffi::PonkoEventTarget::PONKO_EVENT_TO_CLIENTS as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoEventTarget {
    /// To the server (from a client, or the host's own player).
    PONKO_EVENT_TO_SERVER = 0,
    /// Server: to the client `peer`.
    PONKO_EVENT_TO_CLIENT = 1,
    /// Server: to every client that joined.
    PONKO_EVENT_TO_CLIENTS = 2,
}

/// Sends an event of a kind from `ponko_ctx_replication_event`: `size`
/// bytes, reliable and in order (Rust `Replication::to_server_raw`,
/// `to_client_raw`, `to_clients_raw`). `target` is a `PonkoEventTarget`;
/// `peer` is read for `PONKO_EVENT_TO_CLIENT`. Sending to clients from a
/// client does nothing. A kind that wasn't declared, more than
/// Rust `MAX_RAW_EVENT_BYTES` bytes, or 4096 events already waiting to
/// be sent is `PONKO_INVALID_ARGUMENT`, and nothing is sent.
///
/// Ownership: `data` is copied during the call.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_replication_send(std::ptr::null_mut(), 0, 0, 0, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `data` readable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replication_send(
    ctx: *mut PonkoCtx,
    target: u32,
    peer: u64,
    kind: u16,
    data: *const c_void,
    size: usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                let r = &mut c.net.replication;
                let Some(name) = r.raw_event_name(kind).map(str::to_owned) else {
                    return Err(invalid("that kind wasn't declared with ponko_ctx_replication_event"));
                };
                let bytes = slice(data.cast::<u8>(), size, "data")?;
                match target {
                    0 => r.to_server_raw(&name, bytes),
                    1 => r.to_client_raw(PeerId::from_bits(peer), &name, bytes),
                    2 => r.to_clients_raw(&name, bytes),
                    _ => return Err(invalid("target is not a PonkoEventTarget")),
                }
                .map_err(crate::engine_error)
            })
        }
    })
}

/// An event that arrived (`ponko_ctx_net_events`). Its bytes stay valid
/// until the system that read it returns.
/// ```
/// let e = ponko_ffi::PonkoNetEvent { from: 0, tick: 1, kind: 0, data: std::ptr::null(), size: 0 };
/// assert_eq!(e.tick, 1);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoNetEvent {
    /// Who sent it (0: the server, or the host's own player).
    pub from: u64,
    /// The server tick it was sent at.
    pub tick: u64,
    /// Its kind, from `ponko_ctx_replication_event`.
    pub kind: u32,
    /// Its bytes, as sent.
    pub data: *const u8,
    /// How many bytes.
    pub size: usize,
}

/// The events that arrived since this system last read them (Rust
/// `ctx.events.read::<NetEvent<RawEvent>>()`), as an array the engine owns, in
/// order: `out_count` events at `*out_events`. Works inside `ponko_query`
/// callbacks (not `ponko_query_par`); read once per system, as a second
/// read gives only events newer than the first.
///
/// Ownership: `out_events` receives a borrowed view of an array the engine
/// owns, as are the events' bytes, valid until the system returns; never
/// free it, and copy what you keep.
/// ```
/// let (mut e, mut n) = (std::ptr::null(), 0usize);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_net_events(std::ptr::null_mut(), &mut e, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; the outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_net_events(
    ctx: *mut PonkoCtx,
    out_events: *mut *const PonkoNetEvent,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_events.is_null() || out_count.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract. The bytes the array points at
        // live in the events until the next send of their type, which only
        // the replication systems do, between systems.
        unsafe {
            let (ptr, len) = events_kept(ctx, |ev| {
                Ok(ev
                    .read::<NetEvent<RawEvent>>()
                    .iter()
                    .map(|e| PonkoNetEvent {
                        from: e.from.to_bits(),
                        tick: e.tick,
                        kind: u32::from(e.event.kind()),
                        data: e.event.bytes.as_ptr(),
                        size: e.event.bytes.len(),
                    })
                    .collect())
            })?;
            out(out_events, ptr)?;
            out(out_count, len)
        }
    })
}
