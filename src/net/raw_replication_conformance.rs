//! Conformance of replication declared at run time (from C and C++): inputs
//! and prediction over bytes (`raw_input`, `raw_predicted`), events by name
//! (`raw_event`, [`RawNetEvent`]) and interpolated components
//! (`raw_interpolated`), on the scripted network with loss and jitter, the
//! same way the typed declarations are checked in
//! `replication_conformance.rs`.
//!
//! | Guarantee | Test |
//! |---|---|
//! | Raw inputs move the entities their senders own; owners predict them exactly; the server reads each peer's last input | `raw_inputs_predict_exactly_under_loss_and_jitter` |
//! | Raw events arrive once, in order, both ways, with their bytes and sender | `raw_events_arrive_once_and_in_order_both_ways_under_loss` |
//! | A raw interpolated component's floats move smoothly a delay behind the server; its other fields arrive at once | `a_raw_interpolated_component_moves_smoothly_and_sets_other_fields_at_once` |
//! | Misdeclarations panic with a message naming the fix | the `should_panic` tests |

use ponko_ecs::{ComponentId, Entity, World};

use super::replication_conformance::{Session, Side, replicas};
use crate::net::{PeerId, Quantize, RawField, RawNetEvent, RawType, Replica, Replicated, Replication, SnapshotConfig};

/// Registers the run-time components every side uses, in the same order
/// (so their ids agree), and returns (body, pos).
fn register(world: &mut World) -> (ComponentId, ComponentId) {
    // SAFETY: plain bytes, no destructor.
    unsafe {
        (
            world.raw().register_component("RBody", 8, 4, None),
            world.raw().register_component("RPos", 12, 4, None),
        )
    }
}

fn ids() -> (ComponentId, ComponentId) {
    register(&mut World::new())
}

fn floats(bytes: &[u8]) -> (f32, f32) {
    let f = |i: usize| f32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
    (f(0), f(4))
}

fn bytes2(x: f32, y: f32) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&x.to_ne_bytes());
    b[4..].copy_from_slice(&y.to_ne_bytes());
    b
}

/// The C-style step: moves the body by twice the stick, inside a box.
fn walk(body: &mut [u8], stick: &[u8]) {
    let (x, y) = floats(body);
    let (dx, dy) = floats(stick);
    let at = ((x + dx * 2.0).clamp(-500.0, 500.0), (y + dy * 2.0).clamp(-500.0, 500.0));
    body.copy_from_slice(&bytes2(at.0, at.1));
}

fn xy(range: f32) -> Vec<RawField> {
    let q = Quantize::range(-range, range, 0.01);
    vec![
        RawField::new("x", 0, RawType::F32, q),
        RawField::new("y", 4, RawType::F32, q),
    ]
}

fn declarations() -> Replication {
    let (body, pos) = ids();
    let mut pos_fields = xy(2000.0);
    pos_fields.push(RawField::new("hp", 8, RawType::U32, Quantize::Bits(8)));
    Replication::new()
        .raw_component("RBody", body, 8, xy(1000.0))
        .raw_component("RPos", pos, 12, pos_fields)
        .raw_input("RStick", 8, xy(1.0))
        .raw_predicted("RBody", walk)
        .raw_interpolated(
            "RPos",
            SnapshotConfig {
                max_extrapolation: 0.0,
                ..SnapshotConfig::default()
            },
        )
        .raw_event("RShout")
}

fn session(script: impl FnOnce(&mut super::scripted::Network)) -> Session {
    let mut s = Session::scripted(2, declarations, script);
    for side in &mut s.sides {
        assert_eq!(register(&mut side.world), ids(), "the same ids on every side");
    }
    s
}

fn spawn_raw(world: &mut World, id: ComponentId, bytes: &[u8], owner: Option<PeerId>) -> Entity {
    let e = world.spawn((owner.map_or_else(Replicated::new, Replicated::owned_by),));
    // SAFETY: `bytes` is a whole value of the registered size.
    unsafe { world.raw().insert_dynamic(e, id, bytes.as_ptr()) };
    e
}

fn read_raw(world: &World, e: Entity, id: ComponentId, size: usize) -> Vec<u8> {
    let p = world.raw_ref().get_dynamic(e, id).expect("has the component");
    // SAFETY: `p` points to the entity's value of `size` bytes.
    unsafe { std::slice::from_raw_parts(p, size) }.to_vec()
}

#[test]
fn raw_inputs_predict_exactly_under_loss_and_jitter() {
    let mut s = session(|n| {
        n.delay = 3;
        n.jitter = 4;
        n.loss_per_mille = 150;
        n.newest_first = true;
    });
    s.join(400);
    let (body, _) = ids();
    let (a, b) = (s.me(1), s.me(2));
    let world = &mut s.sides[0].world;
    let mine_a = spawn_raw(world, body, &bytes2(0.0, 0.0), Some(a));
    let mine_b = spawn_raw(world, body, &bytes2(50.0, 0.0), Some(b));
    let host = spawn_raw(world, body, &bytes2(-50.0, 0.0), Some(PeerId::HOST));
    s.run(300, |i, side: &mut Side| {
        let t = side.tick as f32 * 0.05;
        let dir = match i {
            0 => (0.0, 1.0),
            1 => (t.cos(), t.sin()),
            _ => (-t.sin(), 0.5),
        };
        side.rep.set_raw_input(&bytes2(dir.0, dir.1));
    });
    let last_b = s.sides[0].rep.raw_input_of(b).map(<[u8]>::to_vec);
    assert_eq!(last_b.map(|v| v.len()), Some(8), "the server reads a peer's last input");
    // Stop pressing (no input: zeroed bytes), let everything land.
    s.run(120, |_, _| {});
    assert_eq!(s.sides[0].rep.raw_input_of(b), Some(&[0u8; 8][..]));
    for (i, side) in s.sides.iter().enumerate().skip(1) {
        let stats = side.rep.client_stats();
        assert_eq!(stats.corrections, 0, "client {i} predicted exactly: {stats:?}");
        assert!(stats.confirmed > 100, "{stats:?}");
    }
    let server = &s.sides[0].world;
    let at = |e| floats(&read_raw(server, e, body, 8));
    assert!(at(mine_a).0.hypot(at(mine_a).1) > 10.0, "client 1 moved");
    assert_ne!(at(mine_b), (50.0, 0.0), "client 2 moved");
    assert_eq!(at(host), (-50.0, 500.0), "the host's own input moved its body");
    // Every client shows the server's values.
    let server_values: Vec<Vec<u8>> = [mine_a, mine_b, host]
        .iter()
        .map(|&e| read_raw(server, e, body, 8))
        .collect();
    for (i, side) in s.sides.iter_mut().enumerate().skip(1) {
        let r = replicas(&mut side.world);
        let mut got: Vec<Vec<u8>> = r.values().map(|&e| read_raw(&side.world, e, body, 8)).collect();
        got.sort();
        let mut want = server_values.clone();
        want.sort();
        assert_eq!(got, want, "client {i} converged");
        let mine = r
            .values()
            .filter(|&&e| side.world.get::<Replica>(e).is_some_and(Replica::is_mine))
            .count();
        assert_eq!(mine, 1, "client {i} owns one");
    }
}

#[test]
fn raw_events_arrive_once_and_in_order_both_ways_under_loss() {
    let mut s = session(|n| {
        n.loss_per_mille = 300;
        n.jitter = 5;
        n.duplicate = true;
    });
    s.join(600);
    let kind = s.sides[0].rep.raw_event_kind("RShout").expect("declared");
    assert_eq!(s.sides[0].rep.raw_event_kind("Nope"), None);
    let mut got: Vec<Vec<(PeerId, u16, u32)>> = vec![Vec::new(); 3];
    let mut read = |i: usize, side: &mut Side| {
        for e in side.events.read::<RawNetEvent>() {
            got[i].push((e.from(), e.kind(), u32::from_le_bytes(e.bytes().try_into().unwrap())));
        }
    };
    for round in 0..40u32 {
        s.step(|i, side| {
            read(i, side);
            match i {
                0 => side.rep.raw_to_clients(kind, &round.to_le_bytes()),
                _ => side.rep.raw_to_server(kind, &(round * 10 + i as u32).to_le_bytes()),
            }
        });
    }
    s.run(200, &mut read);
    for (i, events) in got.iter().enumerate().skip(1) {
        let numbers: Vec<u32> = events.iter().map(|e| e.2).collect();
        assert_eq!(numbers, (0..40).collect::<Vec<_>>(), "client {i}: all, once, in order");
        assert!(events.iter().all(|e| e.0 == PeerId::HOST && e.1 == kind));
    }
    for i in 1..=2u32 {
        let from = s.me(i as usize);
        let numbers: Vec<u32> = got[0].iter().filter(|e| e.0 == from).map(|e| e.2).collect();
        assert_eq!(numbers, (0..40).map(|r| r * 10 + i).collect::<Vec<_>>());
    }
}

#[test]
fn a_raw_interpolated_component_moves_smoothly_and_sets_other_fields_at_once() {
    let mut s = session(|n| {
        n.delay = 2;
        n.jitter = 2;
    });
    s.join(200);
    let (_, pos) = ids();
    let mut value = [0u8; 12];
    let e = spawn_raw(&mut s.sides[0].world, pos, &value, None);
    // The server moves it 1 unit a tick along x and sets hp now and then.
    let mut shown: Vec<(f32, u32)> = Vec::new();
    let mut hp_at_server = 0;
    for tick in 0..240u32 {
        let x = tick as f32;
        let hp = tick / 30;
        value[..4].copy_from_slice(&x.to_ne_bytes());
        value[8..].copy_from_slice(&hp.to_ne_bytes());
        let p = s.sides[0].world.raw().get_dynamic_mut(e, pos).unwrap();
        // SAFETY: the entity's live value of 12 bytes.
        unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), p, 12) };
        s.sides[0].world.raw().mark_changed_dynamic(e, pos);
        hp_at_server = hp;
        s.step(|_, _| {});
        let client = &mut s.sides[1];
        if let Some(&r) = replicas(&mut client.world).values().next() {
            let v = read_raw(&client.world, r, pos, 12);
            shown.push((floats(&v).0, u32::from_ne_bytes(v[8..].try_into().unwrap())));
        }
    }
    s.run(60, |_, _| {});
    let tail = &shown[60..];
    for w in tail.windows(2) {
        let step = w[1].0 - w[0].0;
        assert!((-0.01..=2.5).contains(&step), "smooth, never backwards: {w:?}");
    }
    let lag = 239.0 - tail.last().unwrap().0;
    assert!(lag > 1.0, "shown a little in the past: {lag}");
    // hp isn't interpolated: it is the server's within the network delay.
    let (late_x, late_hp) = *tail.last().unwrap();
    assert!(
        late_hp + 1 >= hp_at_server,
        "hp {late_hp} vs {hp_at_server} at x {late_x}"
    );
    let client = &mut s.sides[1];
    let r = *replicas(&mut client.world).values().next().unwrap();
    let settled = read_raw(&client.world, r, pos, 12);
    assert_eq!(floats(&settled).0, 239.0, "it ends where the server stopped");
}

mod misdeclarations {
    use super::*;

    #[test]
    #[should_panic(expected = "no component `RBody` was declared with raw_component")]
    fn predicting_an_undeclared_component_is_refused() {
        let _ = Replication::new()
            .raw_input("S", 8, xy(1.0))
            .raw_predicted("RBody", walk);
    }

    #[test]
    #[should_panic(expected = "declare the input with raw_input first")]
    fn predicting_without_a_raw_input_is_refused() {
        let (body, _) = ids();
        let _ = Replication::new()
            .raw_component("RBody", body, 8, xy(1000.0))
            .raw_predicted("RBody", walk);
    }

    #[test]
    #[should_panic(expected = "a game has one input type (put every control in it)")]
    fn a_second_raw_input_is_refused() {
        let _ = Replication::new().raw_input("A", 8, xy(1.0)).raw_input("B", 8, xy(1.0));
    }

    #[test]
    #[should_panic(expected = "field `y` of `S` lies outside its 6 bytes")]
    fn a_raw_input_field_outside_its_size_is_refused() {
        let _ = Replication::new().raw_input("S", 6, xy(1.0));
    }

    #[test]
    #[should_panic(expected = "event `E` is declared twice; declare each event once")]
    fn a_raw_event_declared_twice_is_refused() {
        let _ = Replication::new().raw_event("E").raw_event("E");
    }

    #[test]
    #[should_panic(expected = "event kind 3 wasn't declared with `raw_event`")]
    fn sending_an_undeclared_raw_event_kind_is_refused() {
        Replication::new().raw_event("E").raw_to_server(3, &[1]);
    }

    #[test]
    #[should_panic(expected = "the input wasn't declared with raw_input")]
    fn a_raw_input_without_its_declaration_is_refused() {
        Replication::new().raw_event("E").set_raw_input(&[0; 8]);
    }

    #[test]
    #[should_panic(expected = "no component `RPos` was declared with raw_component")]
    fn interpolating_an_undeclared_component_is_refused() {
        let _ = Replication::new().raw_interpolated("RPos", SnapshotConfig::default());
    }
}
