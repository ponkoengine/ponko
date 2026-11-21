//! Replication inputs, prediction, events, interpolation and the interest
//! position from C: a server game and a client game made through the C
//! functions, over the in-memory loopback, taking turns frame by frame.
//! The client's own body is predicted from its inputs, a mover is shown
//! between the values received, events travel both ways, and an entity
//! far from the client's never reaches it.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use PonkoStatus::*;

const TICKS: u64 = 200;
/// Inputs and the mover stop here, so the end state is settled.
const STILL: u64 = 120;

/// `{ float x, y; }`: what the client's input moves.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Body {
    x: f32,
    y: f32,
}

/// `{ float x, y, angle; uint32_t hp; }`: where a thing is, for interest
/// too, and how it is shown on clients.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pos {
    x: f32,
    y: f32,
    angle: f32,
    hp: u32,
}

/// `{ float dx, dy; }`: the input.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Stick {
    dx: f32,
    dy: f32,
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

#[derive(Default)]
struct Side {
    turns: Option<Arc<Turns>>,
    me: usize,
    body: u32,
    pos: u32,
    chat: u16,
    shout: u16,
    owned: u64,
    mover: u64,
    /// Events read, in order: (kind, from, bytes).
    events: Vec<(u16, u64, Vec<u8>)>,
    /// The client's view of the mover, frame by frame.
    mover_x: Vec<f32>,
    /// What this side holds at the end, by net id.
    bodies: Vec<(u32, Body)>,
    positions: Vec<(u32, Pos)>,
    /// Server: the client's input seen mid-run and at the end.
    input_moving: Option<Stick>,
    input_end: Option<Stick>,
    late: Option<PonkoStatus>,
}

static STEPS: AtomicUsize = AtomicUsize::new(0);

/// The game's step: the body moves a tenth of the stick per input.
unsafe extern "C" fn walk(
    state: *mut c_void,
    state_size: usize,
    input: *const c_void,
    input_size: usize,
    _: *mut c_void,
) {
    assert_eq!((state_size, input_size), (8, 8));
    STEPS.fetch_add(1, Ordering::Relaxed);
    unsafe {
        let body = &mut *state.cast::<Body>();
        let stick = *input.cast::<Stick>();
        body.x += stick.dx * 0.1;
        body.y += stick.dy * 0.1;
    }
}

fn f(
    name: &'static std::ffi::CStr,
    offset: usize,
    quantize: PonkoFieldQuantize,
    min: f32,
    max: f32,
) -> PonkoReplicatedField {
    PonkoReplicatedField {
        name: name.as_ptr(),
        offset,
        ty: PonkoReplicatedType::PONKO_REPLICATED_F32,
        quantize,
        min,
        max,
        step: 0.01,
        bits: 16,
    }
}

fn exact(name: &'static std::ffi::CStr, offset: usize) -> PonkoReplicatedField {
    f(name, offset, PonkoFieldQuantize::PONKO_FIELD_EXACT, 0.0, 0.0)
}

/// Declares everything, the same on both sides; returns the event kinds.
unsafe fn declare(ctx: *mut PonkoCtx, side: &mut Side) {
    use PonkoFieldQuantize::*;
    let body = [exact(c"x", 0), exact(c"y", 4)];
    let mut hp = exact(c"hp", 12);
    hp.ty = PonkoReplicatedType::PONKO_REPLICATED_U32;
    let pos = [
        f(c"x", 0, PONKO_FIELD_RANGE, -10_000.0, 10_000.0),
        f(c"y", 4, PONKO_FIELD_RANGE, -10_000.0, 10_000.0),
        f(c"angle", 8, PONKO_FIELD_ANGLE, 0.0, 0.0),
        hp,
    ];
    let components = [
        PonkoReplicatedComponent {
            name: c"body".as_ptr(),
            component: side.body,
            fields: body.as_ptr(),
            field_count: body.len(),
        },
        PonkoReplicatedComponent {
            name: c"pos".as_ptr(),
            component: side.pos,
            fields: pos.as_ptr(),
            field_count: pos.len(),
        },
    ];
    let rings = ponko_interest_config_default();
    let stick = [exact(c"dx", 0), exact(c"dy", 4)];
    // Between two received values, not on one: the delay isn't a whole
    // number of ticks.
    let mut shown = ponko_snapshot_config_default();
    shown.delay = 0.105;
    shown.max_extrapolation = 0.0;
    unsafe {
        assert_eq!(ponko_ctx_replication(ctx, components.as_ptr(), 2, 0, &rings), PONKO_OK);
        assert_eq!(
            ponko_ctx_replication_input(ctx, c"Stick".as_ptr(), 8, stick.as_ptr(), 2),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_replication_predicted(ctx, c"body".as_ptr(), Some(walk), null_mut(), None),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_replication_interpolated(ctx, c"pos".as_ptr(), &shown),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_replication_event(ctx, c"Chat".as_ptr(), &mut side.chat),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_replication_event(ctx, c"Shout".as_ptr(), &mut side.shout),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_replication_position(ctx, side.pos, 0, 2), PONKO_OK);
    }
}

unsafe fn spawn(ctx: *mut PonkoCtx, values: &[PonkoValue], owner: Option<u64>) -> u64 {
    unsafe {
        let mut e = 0;
        assert_eq!(
            ponko_spawn(ponko_ctx_world(ctx), values.as_ptr(), values.len(), &mut e),
            PONKO_OK
        );
        let mut how = ponko_replicated_default();
        if let Some(owner) = owner {
            how.has_owner = true;
            how.owner = owner;
        }
        assert_eq!(ponko_ctx_replicate(ctx, e, &how), PONKO_OK);
        e
    }
}

fn value<T>(component: u32, v: &T) -> PonkoValue {
    PonkoValue {
        component,
        data: (v as *const T).cast(),
        size: size_of::<T>(),
    }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let side = &mut *user.cast::<Side>();
        let world = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_component_register(world, c"body".as_ptr(), 8, 4, &mut side.body),
            PONKO_OK
        );
        assert_eq!(
            ponko_component_register(world, c"pos".as_ptr(), 16, 4, &mut side.pos),
            PONKO_OK
        );
        declare(ctx, side);
        if side.me == 0 {
            // The client's own body, where it looks from.
            let (b, p) = (
                Body::default(),
                Pos {
                    hp: 1,
                    ..Pos::default()
                },
            );
            side.owned = spawn(ctx, &[value(side.body, &b), value(side.pos, &p)], Some(1));
            // A mover near it, and one far away.
            let near = Pos {
                x: 5.0,
                hp: 7,
                ..Pos::default()
            };
            side.mover = spawn(ctx, &[value(side.pos, &near)], None);
            let far = Pos {
                x: 5000.0,
                y: 5000.0,
                hp: 9,
                ..Pos::default()
            };
            spawn(ctx, &[value(side.pos, &far)], None);
        }
        side.turns.as_ref().unwrap().wait(side.me);
    }
}

unsafe fn get<T: Default>(ctx: *mut PonkoCtx, component: u32, e: u64) -> Option<T> {
    let mut v = T::default();
    let status = unsafe { ponko_get(ponko_ctx_world(ctx), e, component, (&raw mut v).cast(), size_of::<T>()) };
    (status == PONKO_OK).then_some(v)
}

unsafe extern "C" fn frame(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let side = &mut *user.cast::<Side>();
        let tick = ponko_ctx_sim_tick(ctx);
        if tick == 1 {
            side.late = Some(ponko_ctx_replication_event(ctx, c"Late".as_ptr(), &mut 0));
        }
        // Events that arrived, read once.
        let (mut events, mut n) = (null(), 0usize);
        assert_eq!(ponko_ctx_net_events(ctx, &mut events, &mut n), PONKO_OK);
        for e in if n > 0 {
            std::slice::from_raw_parts(events, n)
        } else {
            &[]
        } {
            let bytes = std::slice::from_raw_parts(e.data, e.size).to_vec();
            side.events.push((e.kind as u16, e.from, bytes));
        }
        if side.me == 1 {
            let stick = if tick < STILL {
                Stick { dx: 1.0, dy: 0.5 }
            } else {
                Stick::default()
            };
            assert_eq!(
                ponko_ctx_set_replication_input(ctx, (&raw const stick).cast(), 8),
                PONKO_OK
            );
            if tick.is_multiple_of(10) && tick <= 100 {
                let b = [tick as u8, 1, 2];
                let s = ponko_ctx_replication_send(
                    ctx,
                    PonkoEventTarget::PONKO_EVENT_TO_SERVER as u32,
                    0,
                    side.chat,
                    b.as_ptr().cast(),
                    3,
                );
                assert_eq!(s, PONKO_OK);
            }
            // The mover as shown, while it moves.
            for id in 0..8 {
                let mut e = 0;
                if ponko_ctx_net_entity(ctx, id, &mut e) == PONKO_OK
                    && get::<Body>(ctx, side.body, e).is_none()
                    && let Some(p) = get::<Pos>(ctx, side.pos, e)
                    && p.hp == 7
                    && tick < STILL
                {
                    side.mover_x.push(p.x);
                }
            }
        } else {
            if tick < STILL {
                let mut p: Pos = get(ctx, side.pos, side.mover).unwrap();
                p.x += 0.25;
                p.angle = (p.angle + 0.5).rem_euclid(std::f32::consts::TAU);
                assert_eq!(
                    ponko_set(ponko_ctx_world(ctx), side.mover, side.pos, (&raw const p).cast(), 16),
                    PONKO_OK
                );
            }
            if tick.is_multiple_of(15) && tick <= 90 {
                let b = [tick as u8];
                let to = if tick == 45 {
                    PonkoEventTarget::PONKO_EVENT_TO_CLIENT
                } else {
                    PonkoEventTarget::PONKO_EVENT_TO_CLIENTS
                };
                assert_eq!(
                    ponko_ctx_replication_send(ctx, to as u32, 1, side.shout, b.as_ptr().cast(), 1),
                    PONKO_OK
                );
            }
            let mut s = Stick::default();
            if tick == 60 && ponko_ctx_replication_input_of(ctx, 1, (&raw mut s).cast(), 8) == PONKO_OK {
                side.input_moving = Some(s);
            }
            if tick == TICKS && ponko_ctx_replication_input_of(ctx, 1, (&raw mut s).cast(), 8) == PONKO_OK {
                side.input_end = Some(s);
            }
        }
        if tick == TICKS {
            for id in 0..8 {
                let mut e = 0;
                if ponko_ctx_net_entity(ctx, id, &mut e) == PONKO_OK {
                    if let Some(b) = get::<Body>(ctx, side.body, e) {
                        side.bodies.push((id, b));
                    }
                    if let Some(p) = get::<Pos>(ctx, side.pos, e) {
                        side.positions.push((id, p));
                    }
                }
            }
        }
        side.turns.as_ref().unwrap().pass(side.me);
        if tick < TICKS {
            side.turns.as_ref().unwrap().wait(side.me);
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
        ponko_world_free(world);
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
    let mut client = Side {
        turns: Some(turns.clone()),
        me: 1,
        ..Side::default()
    };
    let mut server = Side {
        turns: Some(turns),
        me: 0,
        ..Side::default()
    };
    let client = std::thread::spawn(move || {
        run(client_end as *mut PonkoNetMessages, &mut client);
        client
    });
    run(ends[0], &mut server);
    (server, client.join().expect("the client side panicked"))
}

#[test]
fn c_inputs_move_a_predicted_body_the_same_on_server_and_client() {
    let (server, client) = play();
    assert!(STEPS.load(Ordering::Relaxed) > 0, "the C step ran");
    assert_eq!(server.input_moving, Some(Stick { dx: 1.0, dy: 0.5 }));
    assert_eq!(server.input_end, Some(Stick::default()));
    assert_eq!(server.bodies.len(), 1);
    let (_, body) = server.bodies[0];
    assert!(body.x > 5.0 && (body.y - body.x * 0.5).abs() < 1e-3, "{body:?}");
    // The client predicted exactly what the server computed.
    assert_eq!(client.bodies, server.bodies);
    assert_eq!(server.late, Some(PONKO_INVALID_ARGUMENT));
    assert_eq!(client.late, Some(PONKO_INVALID_ARGUMENT));
}

#[test]
fn c_events_arrive_once_and_in_order_both_ways() {
    let (server, client) = play();
    assert_eq!((server.chat, server.shout), (0, 1));
    assert_eq!((client.chat, client.shout), (0, 1));
    let chats: Vec<_> = (1..=10u8).map(|i| (0u16, 1u64, vec![i * 10, 1, 2])).collect();
    assert_eq!(server.events, chats);
    let shouts: Vec<_> = (1..=6u8).map(|i| (1u16, 0u64, vec![i * 15])).collect();
    assert_eq!(client.events, shouts);
}

#[test]
fn a_c_component_is_shown_between_values_and_sets_interest() {
    let (server, client) = play();
    // The far entity never reached the client: interest uses the `pos`
    // component, and the client looks from its own body.
    assert_eq!(server.positions.len(), 3);
    assert_eq!(client.positions.len(), 2);
    assert!(client.positions.iter().all(|(_, p)| p.x < 1000.0));
    // Settled at the end: the same on both sides (to the quantization).
    let mover = |s: &Side| s.positions.iter().find(|(_, p)| p.hp == 7).map(|(_, p)| *p).unwrap();
    let (a, b) = (mover(&server), mover(&client));
    assert!(
        (a.x - b.x).abs() < 0.01 && (a.x - (5.0 + 0.25 * (STILL - 1) as f32)).abs() < 0.01,
        "{a:?} {b:?}"
    );
    // While it moved, the client showed it smoothly and between the values
    // the server sent (those are on a 0.25 grid).
    let xs = &client.mover_x;
    assert!(xs.len() > 60, "{}", xs.len());
    assert!(
        xs.windows(2).all(|w| w[1] >= w[0] - 1e-3 && w[1] - w[0] < 0.5),
        "{xs:?}"
    );
    let between = xs
        .iter()
        .filter(|x| ((*x * 4.0) - (*x * 4.0).round()).abs() > 0.1)
        .count();
    assert!(between > xs.len() / 2, "{xs:?}");
}

/// Declarations that go wrong, in a setup.
unsafe extern "C" fn mistakes(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let out = &mut *user.cast::<Vec<PonkoStatus>>();
        let world = ponko_ctx_world(ctx);
        let (mut body, mut many) = (0, 0);
        assert_eq!(
            ponko_component_register(world, c"body".as_ptr(), 8, 4, &mut body),
            PONKO_OK
        );
        assert_eq!(
            ponko_component_register(world, c"many".as_ptr(), 33 * 4, 4, &mut many),
            PONKO_OK
        );
        let stick = [exact(c"dx", 0), exact(c"dy", 4)];
        // Before ponko_ctx_replication.
        out.push(ponko_ctx_replication_input(
            ctx,
            c"Stick".as_ptr(),
            8,
            stick.as_ptr(),
            2,
        ));
        let fields = [exact(c"x", 0), exact(c"y", 4)];
        let names: Vec<std::ffi::CString> = (0..33)
            .map(|i| std::ffi::CString::new(format!("f{i}")).unwrap())
            .collect();
        let floats: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, n)| PonkoReplicatedField {
                name: n.as_ptr(),
                ..exact(c"x", i * 4)
            })
            .collect();
        let components = [
            PonkoReplicatedComponent {
                name: c"body".as_ptr(),
                component: body,
                fields: fields.as_ptr(),
                field_count: 2,
            },
            PonkoReplicatedComponent {
                name: c"many".as_ptr(),
                component: many,
                fields: floats.as_ptr(),
                field_count: 33,
            },
        ];
        assert_eq!(ponko_ctx_replication(ctx, components.as_ptr(), 2, 0, null()), PONKO_OK);
        // Predicted before the input; an input of no bytes, or past its size.
        out.push(ponko_ctx_replication_predicted(
            ctx,
            c"body".as_ptr(),
            Some(walk),
            null_mut(),
            None,
        ));
        out.push(ponko_ctx_replication_input(ctx, c"Stick".as_ptr(), 0, null(), 0));
        out.push(ponko_ctx_replication_input(
            ctx,
            c"Stick".as_ptr(),
            4,
            stick.as_ptr(),
            2,
        ));
        out.push(ponko_ctx_replication_input(
            ctx,
            c"Stick".as_ptr(),
            8,
            stick.as_ptr(),
            2,
        )); // OK
        out.push(ponko_ctx_replication_input(
            ctx,
            c"Again".as_ptr(),
            8,
            stick.as_ptr(),
            2,
        ));
        // A null step; a component not declared.
        out.push(ponko_ctx_replication_predicted(
            ctx,
            c"body".as_ptr(),
            None,
            null_mut(),
            None,
        ));
        out.push(ponko_ctx_replication_predicted(
            ctx,
            c"nope".as_ptr(),
            Some(walk),
            null_mut(),
            None,
        ));
        // Interpolation: a bad config, an unknown component, 33 floats.
        let mut bad = ponko_snapshot_config_default();
        bad.capacity = 1;
        out.push(ponko_ctx_replication_interpolated(ctx, c"body".as_ptr(), &bad));
        out.push(ponko_ctx_replication_interpolated(ctx, c"nope".as_ptr(), null()));
        out.push(ponko_ctx_replication_interpolated(ctx, c"many".as_ptr(), null()));
        out.push(ponko_ctx_replication_interpolated(ctx, c"body".as_ptr(), null())); // OK
        // Events: no name, twice.
        let mut kind = 9;
        out.push(ponko_ctx_replication_event(ctx, c"".as_ptr(), &mut kind));
        out.push(ponko_ctx_replication_event(ctx, c"Chat".as_ptr(), &mut kind)); // OK
        out.push(ponko_ctx_replication_event(ctx, c"Chat".as_ptr(), &mut kind));
        // Positions: four floats, past the end, an unknown component.
        out.push(ponko_ctx_replication_position(ctx, body, 0, 4));
        out.push(ponko_ctx_replication_position(ctx, body, 4, 2));
        out.push(ponko_ctx_replication_position(ctx, 999, 0, 2));
        // Sending: a kind not declared, a target that isn't one.
        out.push(ponko_ctx_replication_send(ctx, 0, 0, 5, null(), 0));
        out.push(ponko_ctx_replication_send(ctx, 7, 0, kind, null(), 0));
        out.push(ponko_ctx_replication_send(ctx, 0, 0, kind, null(), 0)); // OK
        // No input from a peer yet.
        let mut s = Stick::default();
        out.push(ponko_ctx_replication_input_of(ctx, 1, (&raw mut s).cast(), 8));
        assert_eq!(kind, 0);
    }
}

#[test]
fn bad_raw_declarations_are_refused_with_a_reason() {
    let mut got: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let game = ponko_game_new(c"mistakes".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 1, Some(mistakes), (&raw mut got).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    let bad = PONKO_INVALID_ARGUMENT;
    #[rustfmt::skip]
    let want = vec![
        bad,
        bad, bad, bad, PONKO_OK, bad,
        bad, bad,
        bad, bad, bad, PONKO_OK,
        bad, PONKO_OK, bad,
        bad, bad, bad,
        bad, bad, PONKO_OK,
        PONKO_NOT_FOUND,
    ];
    assert_eq!(got, want);
}

static FREED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn count_free(user: *mut c_void) {
    assert_eq!(user as usize, 42);
    FREED.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn replaced(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let freed = &mut *user.cast::<Vec<usize>>();
        let mut body = 0;
        assert_eq!(
            ponko_component_register(ponko_ctx_world(ctx), c"body".as_ptr(), 8, 4, &mut body),
            PONKO_OK
        );
        let fields = [exact(c"x", 0)];
        let c = PonkoReplicatedComponent {
            name: c"body".as_ptr(),
            component: body,
            fields: fields.as_ptr(),
            field_count: 1,
        };
        assert_eq!(ponko_ctx_replication(ctx, &c, 1, 0, null()), PONKO_OK);
        // Refused: the caller keeps its data.
        let data = 42usize as *mut c_void;
        assert_eq!(
            ponko_ctx_replication_predicted(ctx, c"body".as_ptr(), Some(walk), data, Some(count_free)),
            PONKO_INVALID_ARGUMENT
        );
        freed.push(FREED.load(Ordering::Relaxed));
        assert_eq!(
            ponko_ctx_replication_input(ctx, c"S".as_ptr(), 8, fields.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_replication_predicted(ctx, c"body".as_ptr(), Some(walk), data, Some(count_free)),
            PONKO_OK
        );
        freed.push(FREED.load(Ordering::Relaxed));
        // A new declaration drops the old one, and its step's data.
        assert_eq!(ponko_ctx_replication(ctx, &c, 1, 0, null()), PONKO_OK);
        freed.push(FREED.load(Ordering::Relaxed));
    }
}

#[test]
fn a_predicted_steps_data_is_freed_once_with_its_declaration() {
    let mut freed: Vec<usize> = Vec::new();
    unsafe {
        let game = ponko_game_new(c"replaced".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 1, Some(replaced), (&raw mut freed).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(freed, vec![0, 0, 1]);
}

struct Loops {
    ctx: usize,
    got: Mutex<Vec<PonkoStatus>>,
}

unsafe extern "C" fn each(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let loops = &*user.cast::<Loops>();
        for _ in 0..(*chunk).count {
            let (mut e, mut n) = (null(), 0usize);
            let s = ponko_ctx_net_events(loops.ctx as *mut PonkoCtx, &mut e, &mut n);
            loops.got.lock().unwrap().push(s);
        }
    }
}

/// Reads events inside a query; `_par` is refused.
unsafe extern "C" fn in_loops(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let loops = &mut *user.cast::<Loops>();
        loops.ctx = ctx as usize;
        let world = ponko_ctx_world(ctx);
        let mut tag = 0;
        assert_eq!(
            ponko_component_register(world, c"tag".as_ptr(), 4, 4, &mut tag),
            PONKO_OK
        );
        let v = 1u32;
        let mut e = 0;
        assert_eq!(ponko_spawn(world, &value(tag, &v), 1, &mut e), PONKO_OK);
        assert_eq!(ponko_query(world, &tag, 1, Some(each), user), PONKO_OK);
        assert_eq!(ponko_query_par(world, &tag, 1, Some(each), user), PONKO_OK);
    }
}

#[test]
fn events_are_read_inside_a_query_but_not_a_parallel_one() {
    let mut loops = Loops {
        ctx: 0,
        got: Mutex::new(Vec::new()),
    };
    unsafe {
        let game = ponko_game_new(c"loops".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 1, Some(in_loops), (&raw mut loops).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(*loops.got.lock().unwrap(), vec![PONKO_OK, PONKO_BUSY]);
}
