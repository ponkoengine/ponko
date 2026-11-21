//! Replication helpers from C (Rust's `seq_newer`, `Authority`,
//! `LatestWins`, `SnapshotStream<Pose>` and `Interest`): they sit on top of
//! the byte messages of `ponko_net_messages_*` and never touch the wire, so
//! the game keeps its own packet format.

use ponko::net::Authority;
use ponko::net::AuthorityCheck;
use ponko::net::Interest;
use ponko::net::InterestChange;
use ponko::net::InterestChangeKind;
use ponko::net::InterestConfig;
use ponko::net::InterestEntity;
use ponko::net::InterestTier;
use ponko::net::InterestViewer;
use ponko::net::LatestWins;
use ponko::net::PeerId;
use ponko::net::Pose;
use ponko::net::SnapshotConfig;
use ponko::net::SnapshotPush;
use ponko::net::SnapshotState;
use ponko::net::SnapshotStream;
use ponko::net::UpdateStamp;
use ponko::net::seq_diff;
use ponko::net::seq_newer;
use ponko::{net::MAX_INTEREST_TIERS, prelude::Vec3};

use crate::ctx::{collect, out};
use crate::three::{PonkoQuat, PonkoVec3, quat_from_c, quat_to_c};
use crate::{Failure, PonkoStatus, boundary, invalid, invalid_owned};

fn v3(v: PonkoVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}
fn v3_to_c(v: Vec3) -> PonkoVec3 {
    PonkoVec3 { x: v.x, y: v.y, z: v.z }
}
/// # Safety
/// `ptr` null or readable for `len` values.
unsafe fn slice<'a, T>(ptr: *const T, len: usize, what: &str) -> Result<&'a [T], Failure> {
    if len == 0 {
        Ok(&[])
    } else if ptr.is_null() {
        Err(invalid_owned(format!("{what} is null")))
    } else {
        // SAFETY: forwarded caller contract.
        Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

// ---- sequence numbers -------------------------------------------------------

/// Is `a` newer than `b` among 16-bit sequence numbers that wrap around
/// (Rust `seq_newer`)? Equal numbers, and numbers exactly 32768 apart, are
/// not newer.
/// ```
/// assert!(ponko_ffi::ponko_seq_newer(0, 65535));
/// assert!(!ponko_ffi::ponko_seq_newer(5, 5));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_seq_newer(a: u16, b: u16) -> bool {
    seq_newer(a, b)
}

/// How far `a` is ahead of `b` among wrapping 16-bit sequence numbers:
/// negative when it is behind (Rust `seq_diff`).
/// ```
/// assert_eq!(ponko_ffi::ponko_seq_diff(1, 65535), 2);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_seq_diff(a: u16, b: u16) -> i16 {
    seq_diff(a, b)
}

/// A state update's place in time: the authority `epoch` it was sent under
/// and a wrapping `seq` counter (Rust `UpdateStamp`).
/// ```
/// let s = ponko_ffi::PonkoUpdateStamp { epoch: 1, seq: 0 };
/// assert_eq!(s.epoch, 1);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoUpdateStamp {
    /// Authority epoch, bumped by every handoff.
    pub epoch: u16,
    /// The sender's wrapping update counter.
    pub seq: u16,
}
impl From<PonkoUpdateStamp> for UpdateStamp {
    fn from(s: PonkoUpdateStamp) -> Self {
        UpdateStamp {
            epoch: s.epoch,
            seq: s.seq,
        }
    }
}

/// Is stamp `a` strictly newer than `b`: a newer epoch, or the same epoch
/// and a newer sequence number?
/// ```
/// use ponko_ffi::*;
/// let old = PonkoUpdateStamp { epoch: 0, seq: 900 };
/// assert!(ponko_stamp_newer(PonkoUpdateStamp { epoch: 1, seq: 0 }, old));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_stamp_newer(a: PonkoUpdateStamp, b: PonkoUpdateStamp) -> bool {
    UpdateStamp::from(a).newer_than(b.into())
}

// ---- authority --------------------------------------------------------------

/// Who owns an entity: an owner peer and an epoch counter (Rust
/// `Authority`). A plain value: keep one per entity. Make it with
/// `ponko_authority_new`.
/// ```
/// let a = ponko_ffi::ponko_authority_new(0);
/// assert_eq!((a.owner, a.epoch), (0, 0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoAuthority {
    /// The peer whose updates are accepted (the host is 0).
    pub owner: u64,
    /// Counts handoffs (wrapping).
    pub epoch: u16,
}
impl PonkoAuthority {
    fn get(&self) -> Authority {
        Authority::with_epoch(PeerId::from_bits(self.owner), self.epoch)
    }
    fn set(&mut self, a: Authority) {
        self.owner = a.owner().to_bits();
        self.epoch = a.epoch();
    }
}

/// How an update compares with an authority record (Rust `AuthorityCheck`).
/// ```
/// let _ = ponko_ffi::PonkoAuthorityCheck::PONKO_AUTHORITY_ACCEPT;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAuthorityCheck {
    /// From the owner, in the current epoch: apply it.
    PONKO_AUTHORITY_ACCEPT = 0,
    /// Sent under an older epoch, before a handoff: drop it.
    PONKO_AUTHORITY_STALE = 1,
    /// The current epoch, but not from the owner: drop it.
    PONKO_AUTHORITY_WRONG_OWNER = 2,
    /// Sent under a newer epoch than we know: the handoff announcement
    /// hasn't arrived yet. Drop it or hold it until `ponko_authority_adopt`.
    PONKO_AUTHORITY_AHEAD = 3,
}

/// A record owned by `owner`, at epoch 0.
/// ```
/// assert_eq!(ponko_ffi::ponko_authority_new(3).owner, 3);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_authority_new(owner: u64) -> PonkoAuthority {
    PonkoAuthority { owner, epoch: 0 }
}

/// Gives the entity to `new_owner` and writes the new epoch to announce
/// (send `(new_owner, epoch)` to everyone on a reliable lane).
/// ```
/// let mut a = ponko_ffi::ponko_authority_new(0);
/// let mut epoch = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_authority_handoff(&mut a, 2, &mut epoch) } as u32, 0);
/// assert_eq!((a.owner, epoch), (2, 1));
/// ```
/// # Safety
/// `authority` and `out_epoch` live and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_authority_handoff(
    authority: *mut PonkoAuthority,
    new_owner: u64,
    out_epoch: *mut u16,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let a = unsafe { authority.as_mut() }.ok_or_else(|| invalid("authority is null"))?;
        let mut rust = a.get();
        let epoch = rust.handoff(PeerId::from_bits(new_owner));
        a.set(rust);
        // SAFETY: forwarded caller contract.
        unsafe { out(out_epoch, epoch) }
    })
}

/// Applies a handoff announced by another peer; `*out_changed` says whether
/// it changed the record. Older epochs are ignored; the same epoch with
/// another owner is a race the lower owner id wins, so all peers agree.
/// ```
/// let mut a = ponko_ffi::ponko_authority_new(0);
/// let mut changed = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_authority_adopt(&mut a, 1, 1, &mut changed) } as u32, 0);
/// assert!(changed && a.owner == 1);
/// ```
/// # Safety
/// `authority` and `out_changed` live and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_authority_adopt(
    authority: *mut PonkoAuthority,
    owner: u64,
    epoch: u16,
    out_changed: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let a = unsafe { authority.as_mut() }.ok_or_else(|| invalid("authority is null"))?;
        let mut rust = a.get();
        let changed = rust.adopt(PeerId::from_bits(owner), epoch);
        a.set(rust);
        // SAFETY: forwarded caller contract.
        unsafe { out(out_changed, changed) }
    })
}

/// Judges an update from peer `from` stamped with `epoch`.
/// ```
/// let a = ponko_ffi::ponko_authority_new(0);
/// let mut check = ponko_ffi::PonkoAuthorityCheck::PONKO_AUTHORITY_STALE;
/// assert_eq!(unsafe { ponko_ffi::ponko_authority_check(&a, 0, 0, &mut check) } as u32, 0);
/// assert_eq!(check, ponko_ffi::PonkoAuthorityCheck::PONKO_AUTHORITY_ACCEPT);
/// ```
/// # Safety
/// `authority` live; `out_check` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_authority_check(
    authority: *const PonkoAuthority,
    from: u64,
    epoch: u16,
    out_check: *mut PonkoAuthorityCheck,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let a = unsafe { authority.as_ref() }.ok_or_else(|| invalid("authority is null"))?;
        let check = match a.get().check(PeerId::from_bits(from), epoch) {
            AuthorityCheck::Accept => PonkoAuthorityCheck::PONKO_AUTHORITY_ACCEPT,
            AuthorityCheck::Stale => PonkoAuthorityCheck::PONKO_AUTHORITY_STALE,
            AuthorityCheck::WrongOwner => PonkoAuthorityCheck::PONKO_AUTHORITY_WRONG_OWNER,
            AuthorityCheck::Ahead => PonkoAuthorityCheck::PONKO_AUTHORITY_AHEAD,
        };
        // SAFETY: forwarded caller contract.
        unsafe { out(out_check, check) }
    })
}

// ---- latest wins ------------------------------------------------------------

/// A "latest wins per entity" filter for unreliable state updates (Rust
/// `LatestWins`). Free it with `ponko_latest_wins_free`.
pub struct PonkoLatestWins(LatestWins<u64>);

/// An empty filter.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_latest_wins_free.
///
/// ```
/// let f = ponko_ffi::ponko_latest_wins_new();
/// unsafe { ponko_ffi::ponko_latest_wins_free(f) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_latest_wins_new() -> *mut PonkoLatestWins {
    Box::into_raw(Box::new(PonkoLatestWins(LatestWins::new())))
}

/// Frees a filter. Null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_latest_wins_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `filter` null or live, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_latest_wins_free(filter: *mut PonkoLatestWins) {
    if !filter.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(filter) });
    }
}

fn filter<'a>(f: *mut PonkoLatestWins) -> Result<&'a mut LatestWins<u64>, Failure> {
    // SAFETY: callers pass null or a live filter.
    unsafe { f.as_mut() }
        .map(|f| &mut f.0)
        .ok_or_else(|| invalid("filter is null"))
}

/// Records `stamp` for `key` (an entity id) and writes whether the update is
/// newer than every one accepted before for that key. Drop it if not.
/// ```
/// use ponko_ffi::*;
/// let f = ponko_latest_wins_new();
/// let mut ok = false;
/// unsafe {
///     ponko_latest_wins_accept(f, 7, PonkoUpdateStamp { epoch: 0, seq: 2 }, &mut ok);
///     assert!(ok);
///     ponko_latest_wins_accept(f, 7, PonkoUpdateStamp { epoch: 0, seq: 1 }, &mut ok);
///     assert!(!ok);
///     ponko_latest_wins_free(f);
/// }
/// ```
/// # Safety
/// `filter` live; `out_accepted` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_latest_wins_accept(
    f: *mut PonkoLatestWins,
    key: u64,
    stamp: PonkoUpdateStamp,
    out_accepted: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        let accepted = filter(f)?.accept(key, stamp.into());
        // SAFETY: forwarded caller contract.
        unsafe { out(out_accepted, accepted) }
    })
}

/// Forgets `key` (the entity is gone): its next update is accepted.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_latest_wins_forget(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `filter` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_latest_wins_forget(f: *mut PonkoLatestWins, key: u64) -> PonkoStatus {
    boundary(|| {
        filter(f)?.forget(&key);
        Ok(())
    })
}

/// Forgets every key.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_latest_wins_clear(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `filter` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_latest_wins_clear(f: *mut PonkoLatestWins) -> PonkoStatus {
    boundary(|| {
        filter(f)?.clear();
        Ok(())
    })
}

/// How many keys the filter remembers (0 for null).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_latest_wins_len(std::ptr::null()) }, 0);
/// ```
/// # Safety
/// `filter` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_latest_wins_len(f: *const PonkoLatestWins) -> usize {
    // SAFETY: forwarded caller contract.
    unsafe { f.as_ref() }.map_or(0, |f| f.0.len())
}

// ---- snapshots --------------------------------------------------------------

/// A position, rotation and optional velocity (Rust `Pose`).
/// ```
/// let p = ponko_ffi::PonkoPose::default();
/// assert!(!p.has_velocity);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoPose {
    /// Where, in world units.
    pub position: PonkoVec3,
    /// Which way it faces.
    pub rotation: PonkoQuat,
    /// Units per second; only read when `has_velocity`. With velocities on
    /// both sides, the path between snapshots is a smooth curve instead of
    /// a straight line.
    pub velocity: PonkoVec3,
    /// Whether `velocity` is meaningful.
    pub has_velocity: bool,
}
impl Default for PonkoPose {
    fn default() -> Self {
        Self {
            position: PonkoVec3::default(),
            rotation: quat_to_c(ponko::prelude::Quat::IDENTITY),
            velocity: PonkoVec3::default(),
            has_velocity: false,
        }
    }
}
fn pose_from_c(p: &PonkoPose) -> Pose {
    Pose {
        position: v3(p.position),
        rotation: quat_from_c(p.rotation),
        velocity: p.has_velocity.then(|| v3(p.velocity)),
    }
}
fn pose_to_c(p: Pose) -> PonkoPose {
    PonkoPose {
        position: v3_to_c(p.position),
        rotation: quat_to_c(p.rotation),
        velocity: v3_to_c(p.velocity.unwrap_or_default()),
        has_velocity: p.velocity.is_some(),
    }
}

/// Tuning of a snapshot stream (Rust `SnapshotConfig`). Start from
/// `ponko_snapshot_config_default`.
/// ```
/// let c = ponko_ffi::ponko_snapshot_config_default();
/// assert_eq!(c.capacity, 64);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoSnapshotConfig {
    /// Seconds in the past that remote entities are shown (default 0.1):
    /// the gap between packets plus their jitter.
    pub delay: f64,
    /// Seconds to predict past the newest snapshot before holding still
    /// (default 0.25).
    pub max_extrapolation: f64,
    /// Snapshots kept, at least 2 (default 64).
    pub capacity: u32,
}

/// The defaults: 0.1 s delay, 0.25 s extrapolation, 64 snapshots.
/// ```
/// assert_eq!(ponko_ffi::ponko_snapshot_config_default().delay, 0.1);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_snapshot_config_default() -> PonkoSnapshotConfig {
    let c = SnapshotConfig::default();
    PonkoSnapshotConfig {
        delay: c.delay,
        max_extrapolation: c.max_extrapolation,
        capacity: c.capacity as u32,
    }
}

/// What happened to a pushed snapshot (Rust `SnapshotPush`).
/// ```
/// let _ = ponko_ffi::PonkoSnapshotPush::PONKO_SNAPSHOT_ADDED;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoSnapshotPush {
    /// Stored, in time order even if it was late.
    PONKO_SNAPSHOT_ADDED = 0,
    /// A snapshot with this timestamp is already stored: ignored.
    PONKO_SNAPSHOT_DUPLICATE = 1,
    /// Older than everything kept in a full buffer: ignored.
    PONKO_SNAPSHOT_TOO_OLD = 2,
    /// The timestamp was not a finite number: ignored.
    PONKO_SNAPSHOT_INVALID = 3,
}

/// How a sampled pose was produced (Rust `SnapshotState`).
/// ```
/// let _ = ponko_ffi::PonkoSnapshotState::PONKO_SNAPSHOT_INTERPOLATED;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoSnapshotState {
    /// Between two snapshots: the normal case.
    PONKO_SNAPSHOT_INTERPOLATED = 0,
    /// Past the newest snapshot, predicted from it for a bounded time.
    PONKO_SNAPSHOT_EXTRAPOLATED = 1,
    /// Standing still: before the oldest snapshot, or past the prediction limit.
    PONKO_SNAPSHOT_HELD = 2,
}

/// The snapshots of one remote entity plus an estimate of the sender's clock
/// (Rust `SnapshotStream<Pose>`). Free it with `ponko_snapshot_stream_free`.
pub struct PonkoSnapshotStream(SnapshotStream<Pose>);

fn stream<'a>(s: *const PonkoSnapshotStream) -> Result<&'a SnapshotStream<Pose>, Failure> {
    // SAFETY: callers pass null or a live stream.
    unsafe { s.as_ref() }
        .map(|s| &s.0)
        .ok_or_else(|| invalid("stream is null"))
}
fn stream_mut<'a>(s: *mut PonkoSnapshotStream) -> Result<&'a mut SnapshotStream<Pose>, Failure> {
    // SAFETY: callers pass null or a live stream.
    unsafe { s.as_mut() }
        .map(|s| &mut s.0)
        .ok_or_else(|| invalid("stream is null"))
}

/// Makes a stream from `*config` (null for the defaults). Fails on a
/// `capacity` below 2 or a negative or non-finite time.
///
/// Ownership: on success `out_stream` receives an owned stream, the caller's;
/// free it with ponko_snapshot_stream_free.
///
/// ```
/// let mut s = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_snapshot_stream_new(std::ptr::null(), &mut s) } as u32, 0);
/// unsafe { ponko_ffi::ponko_snapshot_stream_free(s) };
/// ```
/// # Safety
/// `config` null or readable; `out_stream` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_new(
    config: *const PonkoSnapshotConfig,
    out_stream: *mut *mut PonkoSnapshotStream,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let config = match unsafe { config.as_ref() } {
            Some(c) => SnapshotConfig {
                delay: c.delay,
                max_extrapolation: c.max_extrapolation,
                capacity: c.capacity as usize,
            },
            None => SnapshotConfig::default(),
        };
        config.validate().map_err(invalid)?;
        let stream = Box::into_raw(Box::new(PonkoSnapshotStream(SnapshotStream::with_config(config))));
        // SAFETY: forwarded caller contract.
        unsafe { out(out_stream, stream) }
    })
}

/// Frees a stream. Null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_snapshot_stream_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `stream` null or live, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_free(s: *mut PonkoSnapshotStream) {
    if !s.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(s) });
    }
}

/// Stores a snapshot stamped `remote_time` by the sender (seconds on its
/// clock) that arrived at `local_now` (seconds on yours), and updates the
/// clock estimate. Snapshots may come in any order and more than once.
/// ```
/// use ponko_ffi::*;
/// let mut s = std::ptr::null_mut();
/// let mut result = PonkoSnapshotPush::PONKO_SNAPSHOT_INVALID;
/// unsafe {
///     ponko_snapshot_stream_new(std::ptr::null(), &mut s);
///     ponko_snapshot_stream_push(s, 10.0, 0.5, &PonkoPose::default(), &mut result);
///     assert_eq!(result, PonkoSnapshotPush::PONKO_SNAPSHOT_ADDED);
///     ponko_snapshot_stream_free(s);
/// }
/// ```
/// # Safety
/// `stream` live; `pose` readable; `out_result` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_push(
    s: *mut PonkoSnapshotStream,
    remote_time: f64,
    local_now: f64,
    pose: *const PonkoPose,
    out_result: *mut PonkoSnapshotPush,
) -> PonkoStatus {
    boundary(|| {
        let stream = stream_mut(s)?;
        // SAFETY: forwarded caller contract.
        let pose = unsafe { pose.as_ref() }.ok_or_else(|| invalid("pose is null"))?;
        let result = match stream.push(remote_time, local_now, pose_from_c(pose)) {
            SnapshotPush::Added => PonkoSnapshotPush::PONKO_SNAPSHOT_ADDED,
            SnapshotPush::Duplicate => PonkoSnapshotPush::PONKO_SNAPSHOT_DUPLICATE,
            SnapshotPush::TooOld => PonkoSnapshotPush::PONKO_SNAPSHOT_TOO_OLD,
            SnapshotPush::Invalid => PonkoSnapshotPush::PONKO_SNAPSHOT_INVALID,
        };
        if out_result.is_null() {
            return Ok(());
        }
        // SAFETY: checked non-null.
        unsafe { out_result.write(result) };
        Ok(())
    })
}

/// The pose to show at your time `now`: the sender's time at `now - delay`,
/// interpolated, predicted for a bounded time past the newest snapshot, then
/// held. `*out_found` is false (and nothing else is written) until the first
/// snapshot arrives. `out_state` may be null.
/// ```
/// use ponko_ffi::*;
/// let mut s = std::ptr::null_mut();
/// let (mut pose, mut state, mut found) = (PonkoPose::default(), PonkoSnapshotState::PONKO_SNAPSHOT_HELD, true);
/// unsafe {
///     ponko_snapshot_stream_new(std::ptr::null(), &mut s);
///     ponko_snapshot_stream_sample(s, 0.0, &mut pose, &mut state, &mut found);
///     assert!(!found);
///     ponko_snapshot_stream_free(s);
/// }
/// ```
/// # Safety
/// `stream` live; `out_pose` and `out_found` writable; `out_state` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_sample(
    s: *const PonkoSnapshotStream,
    now: f64,
    out_pose: *mut PonkoPose,
    out_state: *mut PonkoSnapshotState,
    out_found: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        let sample = stream(s)?.sample(now);
        // SAFETY: forwarded caller contract.
        unsafe { out(out_found, sample.is_some())? };
        let Some(sample) = sample else { return Ok(()) };
        // SAFETY: forwarded caller contract.
        unsafe { out(out_pose, pose_to_c(sample.value))? };
        if !out_state.is_null() {
            let state = match sample.state {
                SnapshotState::Interpolated => PonkoSnapshotState::PONKO_SNAPSHOT_INTERPOLATED,
                SnapshotState::Extrapolated => PonkoSnapshotState::PONKO_SNAPSHOT_EXTRAPOLATED,
                SnapshotState::Held => PonkoSnapshotState::PONKO_SNAPSHOT_HELD,
            };
            // SAFETY: checked non-null.
            unsafe { out_state.write(state) };
        }
        Ok(())
    })
}

/// Changes how far in the past the entity is shown, in seconds (not negative).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_snapshot_stream_set_delay(std::ptr::null_mut(), 0.2) } as u32, 1);
/// ```
/// # Safety
/// `stream` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_set_delay(s: *mut PonkoSnapshotStream, delay: f64) -> PonkoStatus {
    boundary(|| {
        let stream = stream_mut(s)?;
        if !(delay.is_finite() && delay >= 0.0) {
            return Err(invalid("delay must be a finite number of seconds, 0 or more"));
        }
        stream.set_delay(delay);
        Ok(())
    })
}

/// Forgets the snapshots but keeps the clock estimate: call it on a teleport
/// or respawn, so the entity jumps instead of sliding across.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_snapshot_stream_clear(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `stream` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_clear(s: *mut PonkoSnapshotStream) -> PonkoStatus {
    boundary(|| {
        stream_mut(s)?.clear();
        Ok(())
    })
}

/// Forgets the snapshots and the clock estimate: a new connection.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_snapshot_stream_reset(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `stream` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_reset(s: *mut PonkoSnapshotStream) -> PonkoStatus {
    boundary(|| {
        stream_mut(s)?.reset();
        Ok(())
    })
}

/// Snapshots stored (0 for null).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_snapshot_stream_len(std::ptr::null()) }, 0);
/// ```
/// # Safety
/// `stream` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_len(s: *const PonkoSnapshotStream) -> usize {
    stream(s).map_or(0, |s| s.buffer().len())
}

/// The clock estimate, your time minus the sender's, in seconds; `*out_known`
/// is false until a snapshot arrived.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_snapshot_stream_clock_offset(std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `stream` live; `out_offset` and `out_known` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_snapshot_stream_clock_offset(
    s: *const PonkoSnapshotStream,
    out_offset: *mut f64,
    out_known: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        let offset = stream(s)?.clock().offset();
        // SAFETY: forwarded caller contract.
        unsafe {
            out(out_known, offset.is_some())?;
            out(out_offset, offset.unwrap_or(0.0))
        }
    })
}

// ---- interest ---------------------------------------------------------------

/// One ring of interest (Rust `InterestTier`).
/// ```
/// let t = ponko_ffi::PonkoInterestTier { radius: 40.0, every: 1 };
/// assert_eq!(t.every, 1);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoInterestTier {
    /// Distance from the viewer, in world units.
    pub radius: f32,
    /// Send once every this many ticks (at least 1).
    pub every: u32,
}

/// The rings of an interest manager (Rust `InterestConfig`). Start from
/// `ponko_interest_config_default`.
/// ```
/// let c = ponko_ffi::ponko_interest_config_default();
/// assert_eq!(c.tier_count, 2);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoInterestConfig {
    /// The first `tier_count` rings, nearest first, with increasing radii.
    pub tiers: [PonkoInterestTier; 8],
    /// How many of `tiers` are used, 1 to 8.
    pub tier_count: u32,
    /// An entity changes ring only once it is this far past the edge, so one
    /// standing on a boundary doesn't flicker. Below the first radius.
    pub hysteresis: f32,
    /// Spread the sends of slow rings over different ticks.
    pub stagger: bool,
}

/// The defaults: 40 units every tick, 120 units every 4th tick, hysteresis 5,
/// staggered.
/// ```
/// assert_eq!(ponko_ffi::ponko_interest_config_default().hysteresis, 5.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_interest_config_default() -> PonkoInterestConfig {
    let c = InterestConfig::default();
    let mut tiers = [PonkoInterestTier::default(); MAX_INTEREST_TIERS];
    for (slot, t) in tiers.iter_mut().zip(&c.tiers) {
        *slot = PonkoInterestTier {
            radius: t.radius,
            every: t.every,
        };
    }
    PonkoInterestConfig {
        tiers,
        tier_count: c.tiers.len() as u32,
        hysteresis: c.hysteresis,
        stagger: c.stagger,
    }
}

/// Somebody who looks at the world: a peer and where. A peer may have
/// several; an entity counts by its nearest.
/// ```
/// let v = ponko_ffi::PonkoInterestViewer::default();
/// assert_eq!(v.peer, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoInterestViewer {
    /// Who receives updates (the id from `PONKO_MESSAGE_CONNECTED`).
    pub peer: u64,
    /// Where they look from.
    pub position: PonkoVec3,
}

/// An entity in the world: your id for it (unique) and its position.
/// ```
/// let e = ponko_ffi::PonkoInterestEntity::default();
/// assert_eq!(e.id, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoInterestEntity {
    /// Your identifier.
    pub id: u64,
    /// Where it is.
    pub position: PonkoVec3,
}

/// One entity a peer is interested in (Rust `InterestEntry`).
/// ```
/// let e = ponko_ffi::PonkoInterestEntry::default();
/// assert!(!e.send);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoInterestEntry {
    /// The entity's id.
    pub entity: u64,
    /// Distance to the peer's nearest viewer.
    pub distance: f32,
    /// Its ring, 0 for the nearest.
    pub tier: u32,
    /// Send its state to the peer on this tick (always true on the tick it
    /// comes into interest).
    pub send: bool,
}

/// Whether an entity entered or left a peer's interest.
/// ```
/// let _ = ponko_ffi::PonkoInterestChangeKind::PONKO_INTEREST_ENTERED;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoInterestChangeKind {
    /// Start telling the peer about it (spawn it there).
    PONKO_INTEREST_ENTERED = 0,
    /// The peer no longer needs it (despawn it there), also when the entity
    /// is gone from the world.
    PONKO_INTEREST_LEFT = 1,
}

/// An entity entering or leaving a peer's interest.
/// ```
/// let c = ponko_ffi::PonkoInterestChange { peer: 1, entity: 2, kind: ponko_ffi::PonkoInterestChangeKind::PONKO_INTEREST_LEFT };
/// assert_eq!(c.entity, 2);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoInterestChange {
    /// The peer concerned.
    pub peer: u64,
    /// The entity concerned.
    pub entity: u64,
    /// What happened.
    pub kind: PonkoInterestChangeKind,
}

/// Decides which entities each peer receives, and how often, by distance
/// (Rust `Interest`). Free it with `ponko_interest_free`.
pub struct PonkoInterest {
    interest: Interest,
    viewers: Vec<InterestViewer>,
    entities: Vec<InterestEntity>,
}

fn interest_ref<'a>(i: *const PonkoInterest) -> Result<&'a PonkoInterest, Failure> {
    // SAFETY: callers pass null or a live interest manager.
    unsafe { i.as_ref() }.ok_or_else(|| invalid("interest is null"))
}

/// Makes an interest manager from `*config` (null for the defaults). Fails
/// if the rings are invalid: radii not positive and increasing, an `every`
/// of 0, or a hysteresis not below the first radius.
///
/// Ownership: on success `out_interest` receives an owned interest map, the
/// caller's; free it with ponko_interest_free.
///
/// ```
/// let mut i = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_interest_new(std::ptr::null(), &mut i) } as u32, 0);
/// unsafe { ponko_ffi::ponko_interest_free(i) };
/// ```
/// # Safety
/// `config` null or readable; `out_interest` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_interest_new(
    config: *const PonkoInterestConfig,
    out_interest: *mut *mut PonkoInterest,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let config = match unsafe { config.as_ref() } {
            Some(c) => {
                let count = c.tier_count as usize;
                if !(1..=MAX_INTEREST_TIERS).contains(&count) {
                    return Err(invalid("tier_count must be 1 to 8"));
                }
                InterestConfig {
                    tiers: c.tiers[..count]
                        .iter()
                        .map(|t| InterestTier {
                            radius: t.radius,
                            every: t.every,
                        })
                        .collect(),
                    hysteresis: c.hysteresis,
                    stagger: c.stagger,
                }
            }
            None => InterestConfig::default(),
        };
        config.validate().map_err(invalid)?;
        let interest = Box::into_raw(Box::new(PonkoInterest {
            interest: Interest::new(config),
            viewers: Vec::new(),
            entities: Vec::new(),
        }));
        // SAFETY: forwarded caller contract.
        unsafe { out(out_interest, interest) }
    })
}

/// Frees an interest manager. Null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_interest_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `interest` null or live, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_interest_free(i: *mut PonkoInterest) {
    if !i.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(i) });
    }
}

/// Recomputes every peer's interest for send tick `tick` from the `viewers`
/// and all `entities`. Peers without a viewer are forgotten, their entities
/// reported as left.
/// ```
/// use ponko_ffi::*;
/// let mut i = std::ptr::null_mut();
/// unsafe {
///     ponko_interest_new(std::ptr::null(), &mut i);
///     let viewer = PonkoInterestViewer::default();
///     let near = PonkoInterestEntity { id: 1, position: PonkoVec3 { x: 5.0, y: 0.0, z: 0.0 } };
///     assert_eq!(ponko_interest_update(i, 0, &viewer, 1, &near, 1) as u32, 0);
///     let mut count = 0;
///     ponko_interest_entries(i, 0, std::ptr::null_mut(), 0, &mut count);
///     assert_eq!(count, 1);
///     ponko_interest_free(i);
/// }
/// ```
/// # Safety
/// `interest` live; `viewers` and `entities` readable for their counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_interest_update(
    i: *mut PonkoInterest,
    tick: u64,
    viewers: *const PonkoInterestViewer,
    viewer_count: usize,
    entities: *const PonkoInterestEntity,
    entity_count: usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: callers pass null or a live interest manager.
        let i = unsafe { i.as_mut() }.ok_or_else(|| invalid("interest is null"))?;
        // SAFETY: forwarded caller contract.
        let (vs, es) = unsafe {
            (
                slice(viewers, viewer_count, "viewers")?,
                slice(entities, entity_count, "entities")?,
            )
        };
        i.viewers.clear();
        i.viewers.extend(vs.iter().map(|v| InterestViewer {
            peer: PeerId::from_bits(v.peer),
            position: v3(v.position),
        }));
        i.entities.clear();
        i.entities.extend(es.iter().map(|e| InterestEntity {
            id: e.id,
            position: v3(e.position),
        }));
        i.interest.update(tick, &i.viewers, &i.entities);
        Ok(())
    })
}

/// Everything `peer` is interested in as of the last update: writes up to
/// `capacity` entries and sets `*out_count` to the total. Call with
/// capacity 0 to learn the size.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_interest_entries(std::ptr::null(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `interest` live; `out_entries` writable for `capacity` entries (null if
/// 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_interest_entries(
    i: *const PonkoInterest,
    peer: u64,
    out_entries: *mut PonkoInterestEntry,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let i = interest_ref(i)?;
        let mut n = 0;
        // SAFETY: forwarded caller contract.
        unsafe {
            collect(out_entries, capacity, out_count, &mut n, |sink| {
                for e in i.interest.entries(PeerId::from_bits(peer)) {
                    sink.push(PonkoInterestEntry {
                        entity: e.entity,
                        distance: e.distance,
                        tier: u32::from(e.tier),
                        send: e.send,
                    });
                }
            })
        }
    })
}

/// The entities that entered or left a peer's interest in the last update:
/// writes up to `capacity` changes and sets `*out_count` to the total.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_interest_changes(std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `interest` live; `out_changes` writable for `capacity` changes (null if
/// 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_interest_changes(
    i: *const PonkoInterest,
    out_changes: *mut PonkoInterestChange,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let i = interest_ref(i)?;
        let mut n = 0;
        // SAFETY: forwarded caller contract.
        unsafe {
            collect(out_changes, capacity, out_count, &mut n, |sink| {
                for &InterestChange { peer, entity, kind } in i.interest.changes() {
                    sink.push(PonkoInterestChange {
                        peer: peer.to_bits(),
                        entity,
                        kind: match kind {
                            InterestChangeKind::Entered => PonkoInterestChangeKind::PONKO_INTEREST_ENTERED,
                            InterestChangeKind::Left => PonkoInterestChangeKind::PONKO_INTEREST_LEFT,
                        },
                    });
                }
            })
        }
    })
}

/// Forgets a peer that disconnected, without reporting its entities as left.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_interest_remove_peer(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `interest` live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_interest_remove_peer(i: *mut PonkoInterest, peer: u64) -> PonkoStatus {
    boundary(|| {
        // SAFETY: callers pass null or a live interest manager.
        let i = unsafe { i.as_mut() }.ok_or_else(|| invalid("interest is null"))?;
        i.interest.remove_peer(PeerId::from_bits(peer));
        Ok(())
    })
}
