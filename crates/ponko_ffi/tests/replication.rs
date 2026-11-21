//! Declared replication from C: a server game and a client game, both made
//! through the C functions, over the in-memory loopback. The server marks
//! entities of a component registered from C; the client's world gets
//! them, with every declared field. The two games take turns frame by
//! frame and count game time, so the run is the same every time.
use ponko_ffi::*;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use PonkoStatus::*;

const TICKS: u64 = 240;
/// The server stops moving things here, so the end state is settled.
const STILL: u64 = 120;
const UNITS: u8 = 5;

/// A C struct: `{ float x; uint8_t hp; uint8_t hidden; }`, 8 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Unit {
    x: f32,
    hp: u8,
    /// Not declared: it never travels.
    hidden: u8,
}

struct Turns {
    turn: Mutex<usize>,
    changed: Condvar,
}

impl Turns {
    fn wait(&self, me: usize) {
        let mut turn = self.turn.lock().unwrap();
        while *turn != me {
            let (next, waited) = self.changed.wait_timeout(turn, Duration::from_secs(60)).unwrap();
            turn = next;
            assert!(!waited.timed_out() || *turn == me, "the other side stopped");
        }
    }
    fn pass(&self, me: usize) {
        *self.turn.lock().unwrap() = 1 - me;
        self.changed.notify_all();
    }
}

struct Side {
    turns: Arc<Turns>,
    me: usize,
    unit: u32,
    entities: Vec<u64>,
    /// What this side holds at the end, by net id.
    seen: BTreeMap<u32, Unit>,
    status: PonkoReplicationStatus,
    client: PonkoReplicationClientStats,
    peer: PonkoReplicationPeerStats,
    resend: PonkoReplicationResendStats,
    clients: Vec<u64>,
    mine: Option<PonkoReplica>,
    late_declaration: Option<PonkoStatus>,
}

fn field(
    name: &'static std::ffi::CStr,
    offset: usize,
    ty: PonkoReplicatedType,
    q: PonkoFieldQuantize,
) -> PonkoReplicatedField {
    PonkoReplicatedField {
        name: name.as_ptr(),
        offset,
        ty,
        quantize: q,
        min: -100.0,
        max: 100.0,
        step: 0.01,
        bits: 7,
    }
}

fn fields() -> [PonkoReplicatedField; 2] {
    [
        field(
            c"x",
            0,
            PonkoReplicatedType::PONKO_REPLICATED_F32,
            PonkoFieldQuantize::PONKO_FIELD_RANGE,
        ),
        field(
            c"hp",
            4,
            PonkoReplicatedType::PONKO_REPLICATED_U8,
            PonkoFieldQuantize::PONKO_FIELD_BITS,
        ),
    ]
}

unsafe fn declare(ctx: *mut PonkoCtx, unit: u32, fields: &[PonkoReplicatedField]) -> PonkoStatus {
    let component = PonkoReplicatedComponent {
        name: c"unit".as_ptr(),
        component: unit,
        fields: fields.as_ptr(),
        field_count: fields.len(),
    };
    unsafe { ponko_ctx_replication(ctx, &component, 1, 0, null()) }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let side = &mut *user.cast::<Side>();
        let world = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_component_register(world, c"unit".as_ptr(), 8, 4, &mut side.unit),
            PONKO_OK
        );
        assert_eq!(declare(ctx, side.unit, &fields()), PONKO_OK);
        if side.me == 0 {
            for i in 0..UNITS {
                let unit = Unit {
                    x: f32::from(i) * 1.5,
                    hp: 10 + i,
                    hidden: 9,
                };
                let value = PonkoValue {
                    component: side.unit,
                    data: (&raw const unit).cast(),
                    size: 8,
                };
                let mut e = 0;
                assert_eq!(ponko_spawn(world, &value, 1, &mut e), PONKO_OK);
                let mut how = ponko_replicated_default();
                if i == 0 {
                    // The first one belongs to the client (peer 1).
                    how.has_owner = true;
                    how.owner = 1;
                    how.priority = 2.0;
                }
                assert_eq!(ponko_ctx_replicate(ctx, e, &how), PONKO_OK);
                side.entities.push(e);
            }
        }
        side.turns.wait(side.me);
    }
}

unsafe fn unit_of(ctx: *mut PonkoCtx, unit: u32, e: u64) -> Unit {
    let mut value = Unit::default();
    unsafe {
        assert_eq!(
            ponko_get(ponko_ctx_world(ctx), e, unit, (&raw mut value).cast(), 8),
            PONKO_OK
        );
    }
    value
}

/// Each frame, after the tick: the server moves its units until `STILL`;
/// at the end both sides note what they hold. Then the other side's turn.
unsafe extern "C" fn frame(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let side = &mut *user.cast::<Side>();
        let tick = ponko_ctx_sim_tick(ctx);
        if tick == 1 {
            side.late_declaration = Some(declare(ctx, side.unit, &fields()));
        }
        if side.me == 0 && tick < STILL {
            for &e in &side.entities {
                let mut u = unit_of(ctx, side.unit, e);
                u.x += 0.25;
                assert_eq!(
                    ponko_set(ponko_ctx_world(ctx), e, side.unit, (&raw const u).cast(), 8),
                    PONKO_OK
                );
            }
        }
        if tick == TICKS {
            assert_eq!(ponko_ctx_replication_status(ctx, &mut side.status), PONKO_OK);
            for id in 0..64 {
                let mut e = 0;
                match ponko_ctx_net_entity(ctx, id, &mut e) {
                    PONKO_OK => {
                        let mut back = u32::MAX;
                        assert_eq!(ponko_ctx_net_id(ctx, e, &mut back), PONKO_OK);
                        assert_eq!(back, id);
                        side.seen.insert(id, unit_of(ctx, side.unit, e));
                        let mut r = PonkoReplica::default();
                        if ponko_ctx_replica(ctx, e, &mut r) == PONKO_OK && r.mine {
                            side.mine = Some(r);
                        }
                    }
                    status => assert_eq!(status, PONKO_NOT_FOUND),
                }
            }
            if side.me == 0 {
                let mut peers = [0u64; 4];
                let mut n = 0;
                assert_eq!(
                    ponko_ctx_replication_clients(ctx, peers.as_mut_ptr(), peers.len(), &mut n),
                    PONKO_OK
                );
                side.clients = peers[..n].to_vec();
                assert_eq!(
                    ponko_ctx_replication_peer_stats(ctx, peers[0], &mut side.peer),
                    PONKO_OK
                );
                let mut none = PonkoReplicationPeerStats::default();
                assert_eq!(ponko_ctx_replication_peer_stats(ctx, 99, &mut none), PONKO_NOT_FOUND);
                assert_eq!(
                    ponko_ctx_replication_resend_stats(ctx, peers[0], &mut side.resend),
                    PONKO_OK
                );
                let mut none = PonkoReplicationResendStats::default();
                assert_eq!(ponko_ctx_replication_resend_stats(ctx, 99, &mut none), PONKO_NOT_FOUND);
            } else {
                assert_eq!(ponko_ctx_replication_client_stats(ctx, &mut side.client), PONKO_OK);
            }
        }
        side.turns.pass(side.me);
        if tick < TICKS {
            side.turns.wait(side.me);
        }
    }
}

fn run(endpoint: *mut PonkoNetMessages, side: &mut Side) {
    unsafe {
        let game = ponko_game_new(c"side".as_ptr(), 1, 1);
        assert_eq!(ponko_net_messages_game_clock(endpoint), PONKO_OK);
        assert_eq!(ponko_game_messages(game, endpoint), PONKO_OK);
        assert_eq!(ponko_game_headless_frames(game, true), PONKO_OK);
        let user = (side as *mut Side).cast::<c_void>();
        assert_eq!(ponko_game_system(game, c"frame".as_ptr(), Some(frame), user), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, TICKS, Some(setup), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world); // the run took the game
    }
}

fn side(turns: &Arc<Turns>, me: usize) -> Side {
    Side {
        turns: turns.clone(),
        me,
        unit: 0,
        entities: Vec::new(),
        seen: BTreeMap::new(),
        status: PonkoReplicationStatus::default(),
        client: PonkoReplicationClientStats::default(),
        peer: PonkoReplicationPeerStats::default(),
        resend: PonkoReplicationResendStats::default(),
        clients: Vec::new(),
        mine: None,
        late_declaration: None,
    }
}

fn play() -> (Side, Side) {
    let mut ends = [null_mut(); 2];
    assert_eq!(unsafe { ponko_net_messages_loopback(2, ends.as_mut_ptr()) }, PONKO_OK);
    let turns = Arc::new(Turns {
        turn: Mutex::new(0),
        changed: Condvar::new(),
    });
    let client_end = ends[1] as usize;
    let mut client = side(&turns, 1);
    let mut server = side(&turns, 0);
    let client = std::thread::spawn(move || {
        run(client_end as *mut PonkoNetMessages, &mut client);
        client
    });
    run(ends[0], &mut server);
    (server, client.join().expect("the client side panicked"))
}

#[test]
fn a_c_server_replicates_c_components_to_a_c_client() {
    let (server, client) = play();
    assert_eq!(server.status.role, PonkoReplicationRole::PONKO_REPLICATION_SERVER);
    assert!(server.status.joined && server.status.has_me && server.status.me == 0);
    assert_eq!(server.clients, vec![1]);
    assert!(server.peer.creates_sent >= u64::from(UNITS) && server.peer.bytes_sent > 0);
    // The loopback loses nothing: no resends, no backoff, and a timeout of
    // a measured round trip (at least two ticks).
    let r = server.resend;
    assert_eq!((r.early_resends, r.late_acks, r.backoff), (0, 0, 0), "{r:?}");
    assert!(r.timeout_ticks >= 2, "{r:?}");

    assert_eq!(client.status.role, PonkoReplicationRole::PONKO_REPLICATION_CLIENT);
    assert!(client.status.joined && !client.status.refused);
    assert_eq!((client.status.has_me, client.status.me), (true, 1));
    assert!(client.status.server_tick > STILL);
    assert_eq!(client.client.spawned, u64::from(UNITS));
    assert_eq!(client.client.packets_bad, 0);

    // Every declared field arrived; the undeclared one stays zero.
    assert_eq!(server.seen.len(), usize::from(UNITS));
    let want: BTreeMap<u32, Unit> = server
        .seen
        .iter()
        .map(|(&id, u)| (id, Unit { hidden: 0, ..*u }))
        .collect();
    assert_eq!(client.seen, want);
    assert!(server.seen.values().all(|u| u.hidden == 9));
    let mine = client.mine.expect("the client owns one unit");
    assert_eq!((mine.has_owner, mine.owner), (true, 1));
    assert_eq!(client.seen[&mine.id].hp, 10, "the first unit is the client's");
}

#[test]
fn replication_is_declared_before_the_first_tick() {
    let (server, client) = play();
    assert_eq!(server.late_declaration, Some(PONKO_INVALID_ARGUMENT));
    assert_eq!(client.late_declaration, Some(PONKO_INVALID_ARGUMENT));
}

/// Calls `ponko_ctx_replication` in a setup with bad declarations.
unsafe extern "C" fn mistakes(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let out = &mut *user.cast::<Vec<PonkoStatus>>();
        let world = ponko_ctx_world(ctx);
        let mut unit = 0;
        assert_eq!(
            ponko_component_register(world, c"unit".as_ptr(), 8, 4, &mut unit),
            PONKO_OK
        );
        let good = fields();
        // A field past the component's 8 bytes.
        let mut outside = good;
        outside[1].offset = 8;
        out.push(declare(ctx, unit, &outside));
        // A range on an integer.
        let mut ranged_int = good;
        ranged_int[1].quantize = PonkoFieldQuantize::PONKO_FIELD_RANGE;
        out.push(declare(ctx, unit, &ranged_int));
        // An empty range.
        let mut empty = good;
        empty[0].max = empty[0].min;
        out.push(declare(ctx, unit, &empty));
        // Too many bits for a byte.
        let mut wide = good;
        wide[1].bits = 9;
        out.push(declare(ctx, unit, &wide));
        // The same name twice.
        let twice = [PonkoReplicatedComponent {
            name: c"unit".as_ptr(),
            component: unit,
            fields: good.as_ptr(),
            field_count: 2,
        }; 2];
        out.push(ponko_ctx_replication(ctx, twice.as_ptr(), 2, 0, null()));
        // A component this world doesn't have.
        out.push(declare(ctx, 999, &good));
        // Interest rings that don't grow.
        let mut rings = ponko_interest_config_default();
        rings.tiers[1].radius = rings.tiers[0].radius;
        let component = PonkoReplicatedComponent {
            name: c"unit".as_ptr(),
            component: unit,
            fields: good.as_ptr(),
            field_count: 2,
        };
        out.push(ponko_ctx_replication(ctx, &component, 1, 0, &rings));
        // A dead entity can't be marked; an unmarked one has no net id.
        let mut e = 0;
        assert_eq!(ponko_spawn(world, null(), 0, &mut e), PONKO_OK);
        let mut id = 0;
        out.push(ponko_ctx_net_id(ctx, e, &mut id));
        let mut bad = ponko_replicated_default();
        bad.priority = f32::NAN;
        out.push(ponko_ctx_replicate(ctx, e, &bad));
        assert_eq!(ponko_despawn(world, e), PONKO_OK);
        out.push(ponko_ctx_replicate(ctx, e, null()));
        // And the good one still works, with interest rings.
        let rings = ponko_interest_config_default();
        out.push(ponko_ctx_replication(ctx, &component, 1, 32_000, &rings));
        let at = PonkoVec3 { x: 1.0, y: 2.0, z: 0.0 };
        out.push(ponko_ctx_replication_set_viewer(ctx, 1, &at));
        out.push(ponko_ctx_replication_set_viewer(ctx, 1, null()));
    }
}

#[test]
fn bad_declarations_are_refused_with_a_reason() {
    let mut got: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let game = ponko_game_new(c"mistakes".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 1, Some(mistakes), (&raw mut got).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world); // the run took the game
    }
    let invalid = PONKO_INVALID_ARGUMENT;
    assert_eq!(
        got,
        vec![
            invalid,
            invalid,
            invalid,
            invalid,
            invalid,
            invalid,
            invalid,
            PONKO_NOT_FOUND,
            invalid,
            PONKO_NOT_FOUND,
            PONKO_OK,
            PONKO_OK,
            PONKO_OK
        ]
    );
}

#[test]
fn a_game_takes_its_endpoint_even_when_it_fails() {
    let mut ends = [null_mut(); 2];
    unsafe {
        assert_eq!(ponko_net_messages_loopback(2, ends.as_mut_ptr()), PONKO_OK);
        // A null game: the endpoint is freed all the same.
        assert_eq!(ponko_game_messages(null_mut(), ends[0]), PONKO_INVALID_ARGUMENT);
        let game = ponko_game_new(c"g".as_ptr(), 1, 1);
        assert_eq!(ponko_game_messages(game, null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_game_messages(game, ends[1]), PONKO_OK);
        ponko_game_free(game);
    }
}
