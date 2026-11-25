//! The replication helpers on top of `NetMessages`: two endpoints exchange
//! snapshots (with a packet format of the test's own) over a lossy in-memory
//! link with delay and jitter.

use std::time::Duration;

use ponko::net::Authority;
use ponko::net::AuthorityCheck;
use ponko::net::Interest;
use ponko::net::InterestConfig;
use ponko::net::InterestEntity;
use ponko::net::InterestViewer;
use ponko::net::Lane;
use ponko::net::LatestWins;
use ponko::net::MessageEvent;
use ponko::net::NetMessages;
use ponko::net::PeerId;
use ponko::net::Pose;
use ponko::net::SnapshotConfig;
use ponko::net::SnapshotState;
use ponko::net::SnapshotStream;
use ponko::net::UpdateStamp;
use ponko::prelude::{Quat, Vec3, vec3};

const FRAME: f64 = 1.0 / 60.0;
/// The sender's clock runs this far ahead of the receiver's.
const CLOCK_SKEW: f64 = 1000.0;

// ---- a game's own packet format: the engine never sees it ----------------

const STATE: u8 = 1;
const HANDOFF: u8 = 2;

struct State {
    entity: u64,
    stamp: UpdateStamp,
    time: f64,
    pose: Pose,
}

fn encode_state(s: &State) -> Vec<u8> {
    let mut b = vec![STATE];
    b.extend(s.entity.to_le_bytes());
    b.extend(s.stamp.epoch.to_le_bytes());
    b.extend(s.stamp.seq.to_le_bytes());
    b.extend(s.time.to_le_bytes());
    for v in s.pose.position.to_array() {
        b.extend(v.to_le_bytes());
    }
    for v in s.pose.rotation.to_array() {
        b.extend(v.to_le_bytes());
    }
    for v in s.pose.velocity.unwrap_or_default().to_array() {
        b.extend(v.to_le_bytes());
    }
    b
}

fn decode_state(b: &[u8]) -> Option<State> {
    if b.first() != Some(&STATE) || b.len() != 1 + 8 + 2 + 2 + 8 + 40 {
        return None;
    }
    let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    Some(State {
        entity: u64::from_le_bytes(b[1..9].try_into().unwrap()),
        stamp: UpdateStamp {
            epoch: u16::from_le_bytes(b[9..11].try_into().unwrap()),
            seq: u16::from_le_bytes(b[11..13].try_into().unwrap()),
        },
        time: f64::from_le_bytes(b[13..21].try_into().unwrap()),
        pose: Pose {
            position: vec3(f32_at(21), f32_at(25), f32_at(29)),
            rotation: Quat::from_array([f32_at(33), f32_at(37), f32_at(41), f32_at(45)]),
            velocity: Some(vec3(f32_at(49), f32_at(53), f32_at(57))),
        },
    })
}

// ---- helpers --------------------------------------------------------------

fn step(nets: &mut [NetMessages]) {
    for net in nets.iter_mut() {
        net.poll_elapsed(Duration::from_secs_f64(FRAME)).unwrap();
    }
}

fn connect(nets: &mut [NetMessages]) -> PeerId {
    for _ in 0..40 {
        step(nets);
    }
    let guest = nets[0]
        .events()
        .find_map(|e| match e {
            MessageEvent::Connected(p) => Some(p),
            _ => None,
        })
        .expect("the host sees the guest");
    nets[1].events().for_each(drop);
    guest
}

/// A link that holds each packet back for a while before the transport sees
/// it: delay plus jitter, on top of the transport's own loss and reordering.
struct DelayLine {
    held: Vec<(f64, Vec<u8>)>,
}
impl DelayLine {
    fn new() -> Self {
        Self { held: Vec::new() }
    }
    fn send(&mut self, due: f64, bytes: Vec<u8>) {
        self.held.push((due, bytes));
    }
    fn release(&mut self, now: f64, mut out: impl FnMut(Vec<u8>)) {
        let mut i = 0;
        while i < self.held.len() {
            if self.held[i].0 <= now {
                out(self.held.remove(i).1);
            } else {
                i += 1;
            }
        }
    }
}

/// A vehicle driven in a circle of radius 20 at 20 units/s.
fn drive(t: f64) -> Pose {
    let w = 1.0;
    let (s, c) = (t * w).sin_cos();
    Pose {
        position: vec3(20.0 * c as f32, 0.0, 20.0 * s as f32),
        rotation: Quat::from_rotation_y(-(t * w) as f32),
        velocity: Some(vec3(-20.0 * s as f32, 0.0, 20.0 * c as f32)),
    }
}

/// The guest drives entity 42 and streams it to the host, 20 times a second,
/// through delay `delay_ms`+jitter and a link that drops every `drop_one_in`th
/// datagram. Returns what the host would draw each frame, beside the truth.
fn stream_to_host(drop_one_in: u32, config: SnapshotConfig) -> Vec<(f64, Vec3, SnapshotState, Vec3)> {
    let mut nets = NetMessages::loopback_lossy(2, drop_one_in);
    connect(&mut nets);
    let mut line = DelayLine::new();
    let mut filter = LatestWins::new();
    let mut stream = SnapshotStream::<Pose>::with_config(config);
    let (mut seq, mut frames) = (0u16, Vec::new());
    for frame in 0..600u32 {
        let now = frame as f64 * FRAME;
        if frame % 3 == 0 {
            let sent = CLOCK_SKEW + now;
            let jitter = f64::from((frame * 7919) % 41) / 1000.0; // 0–40 ms
            let state = State {
                entity: 42,
                stamp: UpdateStamp { epoch: 0, seq },
                time: sent,
                pose: drive(now),
            };
            seq = seq.wrapping_add(1);
            line.send(now + 0.03 + jitter, encode_state(&state));
        }
        let guest = &mut nets[1];
        line.release(now, |bytes| guest.send(PeerId::HOST, Lane::Unreliable, &bytes).unwrap());
        step(&mut nets);
        let events: Vec<_> = nets[0].events().collect();
        for event in events {
            let MessageEvent::Message { bytes, .. } = event else {
                continue;
            };
            let Some(s) = decode_state(&bytes) else { continue };
            if filter.accept(s.entity, s.stamp) {
                stream.push(s.time, now, s.pose);
            }
        }
        if let Some(sample) = stream.sample(now) {
            // Where the guest was `delay` + the shortest trip (30 ms) ago.
            let truth = drive(now - stream.buffer().config().delay - 0.03).position;
            frames.push((now, sample.value.position, sample.state, truth));
        }
    }
    frames
}

fn worst_error(frames: &[(f64, Vec3, SnapshotState, Vec3)]) -> f32 {
    frames
        .iter()
        .filter(|f| f.0 > 2.0)
        .map(|f| f.1.distance(f.3))
        .fold(0.0, f32::max)
}

fn biggest_jump(frames: &[(f64, Vec3, SnapshotState, Vec3)]) -> f32 {
    frames
        .windows(2)
        .filter(|w| w[0].0 > 2.0)
        .map(|w| w[0].1.distance(w[1].1))
        .fold(0.0, f32::max)
}

// ---- tests ----------------------------------------------------------------

#[test]
fn a_remote_vehicle_is_drawn_smoothly_over_a_clean_link() {
    let frames = stream_to_host(0, SnapshotConfig::default());
    assert!(frames.len() > 500);
    // 20 units/s is 0.33 units per frame; 40 ms of jitter and a 100 ms delay
    // never starve the buffer.
    assert!(biggest_jump(&frames) < 0.6, "{}", biggest_jump(&frames));
    assert!(worst_error(&frames) < 1.5, "{}", worst_error(&frames));
    let late = frames.iter().filter(|f| f.0 > 2.0);
    assert!(late.clone().all(|f| f.2 == SnapshotState::Interpolated));
}

#[test]
fn a_remote_vehicle_survives_packet_loss_and_reordering() {
    // Every 4th datagram lost and the transport delivers newest-first: with
    // a longer delay and Hermite smoothing the picture stays continuous.
    let config = SnapshotConfig {
        delay: 0.15,
        ..Default::default()
    };
    let frames = stream_to_host(4, config);
    assert!(biggest_jump(&frames) < 0.8, "{}", biggest_jump(&frames));
    assert!(worst_error(&frames) < 2.5, "{}", worst_error(&frames));
    // Gaps were bridged by interpolation, extrapolation or a hold, never by
    // an empty answer.
    assert!(frames.len() > 500);
}

#[test]
fn a_total_blackout_extrapolates_briefly_then_holds() {
    let mut nets = NetMessages::loopback(2);
    connect(&mut nets);
    let mut stream = SnapshotStream::<Pose>::new();
    for frame in 0..180u32 {
        let now = frame as f64 * FRAME;
        // The guest stops sending after 1 s.
        if frame % 3 == 0 && now < 1.0 {
            let s = State {
                entity: 1,
                stamp: UpdateStamp::default(),
                time: CLOCK_SKEW + now,
                pose: drive(now),
            };
            nets[1].send(PeerId::HOST, Lane::Unreliable, &encode_state(&s)).unwrap();
        }
        step(&mut nets);
        let events: Vec<_> = nets[0].events().collect();
        for e in events {
            if let MessageEvent::Message { bytes, .. } = e {
                let s = decode_state(&bytes).unwrap();
                stream.push(s.time, now, s.pose);
            }
        }
    }
    let at = |now: f64| stream.sample(now).unwrap();
    assert_eq!(at(0.9).state, SnapshotState::Interpolated);
    assert_eq!(at(1.15).state, SnapshotState::Extrapolated);
    let held_a = at(2.0);
    let held_b = at(2.9);
    assert_eq!(held_a.state, SnapshotState::Held);
    assert_eq!(held_a.value.position, held_b.value.position);
}

#[test]
fn a_handoff_over_the_wire_rejects_late_updates_from_the_old_owner() {
    let mut nets = NetMessages::loopback(2);
    let guest = connect(&mut nets);
    let (host_id, guest_id) = (PeerId::HOST, guest);

    // Both sides track who owns entity 7. The host owns it at first and
    // streams its state; then the host hands it to the guest.
    let mut host_view = Authority::new(host_id);
    let mut guest_view = Authority::new(host_id);
    let mut guest_filter = LatestWins::new();

    let state = |epoch, seq, x: f32| State {
        entity: 7,
        stamp: UpdateStamp { epoch, seq },
        time: 0.0,
        pose: Pose::at(vec3(x, 0.0, 0.0)),
    };
    // Updates 0..3 from the host, epoch 0.
    for seq in 0..3 {
        nets[0]
            .send(guest_id, Lane::Unreliable, &encode_state(&state(0, seq, seq as f32)))
            .unwrap();
        step(&mut nets);
    }
    // Update 3 is sent... and then the handoff happens.
    let late = encode_state(&state(0, 3, 3.0));
    let epoch = host_view.handoff(guest_id);
    let mut announcement = vec![HANDOFF];
    announcement.extend(guest_id.to_bits().to_le_bytes());
    announcement.extend(epoch.to_le_bytes());
    nets[0].send(guest_id, Lane::Reliable, &announcement).unwrap();
    step(&mut nets);
    // The guest's first update as owner overtakes the old owner's last one.
    nets[1]
        .send(host_id, Lane::Unreliable, &encode_state(&state(epoch, 0, 100.0)))
        .unwrap();
    step(&mut nets);
    nets[0].send(guest_id, Lane::Unreliable, &late).unwrap();
    step(&mut nets);

    // The guest applies what it got.
    let mut applied = Vec::new();
    for event in nets[1].events().collect::<Vec<_>>() {
        let MessageEvent::Message { bytes, .. } = event else {
            continue;
        };
        match bytes[0] {
            HANDOFF => {
                let owner = PeerId::from_bits(u64::from_le_bytes(bytes[1..9].try_into().unwrap()));
                let epoch = u16::from_le_bytes(bytes[9..11].try_into().unwrap());
                assert!(guest_view.adopt(owner, epoch));
            }
            STATE => {
                let s = decode_state(&bytes).unwrap();
                if guest_view.check(host_id, s.stamp.epoch) == AuthorityCheck::Accept
                    && guest_filter.accept(s.entity, s.stamp)
                {
                    applied.push(s.pose.position.x);
                }
            }
            _ => unreachable!(),
        }
    }
    // The late update from before the handoff was refused; nothing from the
    // host is applied after it.
    assert!(!applied.contains(&3.0), "{applied:?}");
    assert_eq!(guest_view.owner(), guest_id);

    // The host takes the guest's update as the owner's.
    let mut host_filter = LatestWins::new();
    let mut from_guest = Vec::new();
    for event in nets[0].events().collect::<Vec<_>>() {
        let MessageEvent::Message { bytes, .. } = event else {
            continue;
        };
        let s = decode_state(&bytes).unwrap();
        if host_view.check(guest_id, s.stamp.epoch) == AuthorityCheck::Accept && host_filter.accept(s.entity, s.stamp) {
            from_guest.push(s.pose.position.x);
        }
    }
    assert_eq!(from_guest, [100.0]);
}

#[test]
fn interest_decides_what_each_peer_is_sent() {
    let mut nets = NetMessages::loopback(3);
    for _ in 0..40 {
        step(&mut nets);
    }
    nets[0].events().for_each(drop);
    // Guests say which one they are, so the host knows who is who.
    for (n, net) in nets.iter_mut().enumerate().skip(1) {
        net.events().for_each(drop);
        net.send(PeerId::HOST, Lane::Reliable, &[n as u8]).unwrap();
    }
    for _ in 0..10 {
        step(&mut nets);
    }
    let mut peers = [PeerId::HOST; 2];
    for event in nets[0].events().collect::<Vec<_>>() {
        if let MessageEvent::Message { peer, bytes, .. } = event {
            peers[bytes[0] as usize - 1] = peer;
        }
    }
    assert_ne!(peers[0], peers[1]);

    // 300 entities along a line; the two guests look from opposite ends.
    let world: Vec<_> = (0..300u64)
        .map(|id| InterestEntity {
            id,
            position: vec3(id as f32 * 10.0, 0.0, 0.0),
        })
        .collect();
    let viewers = [
        InterestViewer {
            peer: peers[0],
            position: Vec3::ZERO,
        },
        InterestViewer {
            peer: peers[1],
            position: vec3(3000.0, 0.0, 0.0),
        },
    ];
    let mut interest = Interest::new(InterestConfig::default());
    let mut received: [Vec<u64>; 2] = Default::default();
    let mut collect = |nets: &mut [NetMessages]| {
        for (n, net) in nets[1..].iter_mut().enumerate() {
            for event in net.events().collect::<Vec<_>>() {
                if let MessageEvent::Message { bytes, .. } = event {
                    received[n].push(u64::from_le_bytes(bytes[8..16].try_into().unwrap()));
                }
            }
        }
    };
    for tick in 0..8u64 {
        interest.update(tick, &viewers, &world);
        for &peer in &peers {
            for entry in interest.due(peer) {
                let mut b = tick.to_le_bytes().to_vec();
                b.extend(entry.entity.to_le_bytes());
                nets[0].send(peer, Lane::Reliable, &b).unwrap();
            }
        }
        step(&mut nets);
        collect(&mut nets);
    }
    for _ in 0..10 {
        step(&mut nets);
        collect(&mut nets);
    }

    // (viewer x, an entity next to it, one in the mid ring)
    for (n, (from, near, mid)) in [(0.0f32, 0u64, 11u64), (3000.0, 299, 289)].into_iter().enumerate() {
        let ids = &received[n];
        // Only entities within 120 units of their viewer were ever sent.
        for &id in ids {
            let d = (id as f32 * 10.0 - from).abs();
            assert!(d <= 120.0, "peer {n} was sent entity {id} at distance {d}");
        }
        // The nearest arrives every tick, the mid one on entering and then
        // every 4th tick (staggered by its id): at least twice in 8 ticks.
        assert_eq!(ids.iter().filter(|&&id| id == near).count(), 8, "peer {n}");
        let mid_count = ids.iter().filter(|&&id| id == mid).count();
        assert!((2..=4).contains(&mid_count), "peer {n}: {mid_count}");
        // A dozen entities each (within 115 units, every 10), out of 300.
        let distinct: std::collections::BTreeSet<_> = ids.iter().collect();
        assert!((11..=12).contains(&distinct.len()), "peer {n}: {}", distinct.len());
    }
}

#[cfg(feature = "3d")]
#[test]
fn net_pose_moves_a_transform_from_buffered_snapshots() {
    #[cfg(feature = "3d")]
    use ponko::d3::Transform as Transform3D;
    use ponko::systems::apply_net_poses;
    use ponko::{net::NetPose, prelude::Game};
    // Snapshots a second apart; the headless run's clock advances 1/60 s per tick.
    let mut world = Game::new("remote", 1, 1)
        .fixed_hz(60)
        .sim_system(apply_net_poses)
        .run_headless(90, |ctx| {
            let mut pose = NetPose::default();
            pose.stream.push(CLOCK_SKEW, 0.0, Pose::at(vec3(0.0, 0.0, 0.0)));
            pose.stream.push(
                CLOCK_SKEW + 1.0,
                1.0,
                Pose::at(vec3(100.0, 0.0, 0.0)).facing(Quat::from_rotation_y(1.0)),
            );
            let t = Transform3D {
                scale: vec3(2.0, 2.0, 2.0),
                ..Default::default()
            };
            ctx.world.spawn((t, pose));
        });
    let t = *world.query::<&Transform3D>().iter().next().unwrap();
    // After 1.5 s (90 ticks) the entity is shown at 1.4 s: extrapolated
    // 0.4 s past the last snapshot (capped at 0.25 s), then held.
    assert!(t.position.x > 100.0 && t.position.x <= 126.0, "{}", t.position.x);
    assert_eq!(t.scale, vec3(2.0, 2.0, 2.0), "scale is left alone");
    assert!(t.rotation.angle_between(Quat::from_rotation_y(1.0)) < 1e-4);
}

#[test]
fn a_receiver_maps_the_senders_clock_and_keeps_only_newer_updates() {
    use ponko::net::{ClockSync, SnapshotBuffer};
    // The sender's clock runs `CLOCK_SKEW` ahead; packets spend 50 ms on the way.
    let sent = |seq: u16| CLOCK_SKEW + f64::from(seq) * 0.1;
    let arrived = |seq: u16| f64::from(seq) * 0.1 + 0.05;
    let mut clock = ClockSync::new();
    let mut buffer = SnapshotBuffer::<f32>::new();
    let mut filter = LatestWins::new();
    for seq in 0..10u16 {
        clock.observe(sent(seq), arrived(seq));
        assert!(filter.accept_seq(7u64, seq));
        buffer.push(sent(seq), f32::from(seq));
    }
    assert!(!filter.accept_seq(7, 3), "a late update is dropped");
    assert_eq!(filter.latest(&7).map(|s| s.seq), Some(9));

    let local = clock.to_local(sent(9)).unwrap();
    assert!((local - arrived(9)).abs() < 1e-9, "{local}");
    assert!((clock.to_remote(local).unwrap() - sent(9)).abs() < 1e-9);

    assert_eq!(buffer.oldest_time(), Some(sent(0)));
    assert_eq!(buffer.latest_time(), Some(sent(9)));
    // Halfway between two snapshots, on the sender's timeline.
    let mid = buffer.sample_at((sent(4) + sent(5)) / 2.0).unwrap();
    assert_eq!(mid.state, SnapshotState::Interpolated);
    assert!((mid.value - 4.5).abs() < 1e-3, "{}", mid.value);
    // Once the view has passed snapshot 5, older ones can go.
    buffer.trim_before(sent(5) + 0.01);
    assert_eq!(buffer.oldest_time(), Some(sent(5)));

    // A stream does the clock mapping itself.
    let mut stream = SnapshotStream::<f32>::new();
    stream.push(sent(0), arrived(0), 0.0);
    assert!((stream.latest_local_time().unwrap() - arrived(0)).abs() < 1e-9);
}

#[test]
fn small_integers_and_rotations_replicate_at_their_declared_size() {
    use ponko::net::{Fields, Quantize, Replicate, replicated_bits};
    #[derive(Clone, Default)]
    struct Turret {
        heat: i8,
        aim: Quat,
    }
    impl Replicate for Turret {
        fn fields(&mut self, f: &mut Fields<'_>) {
            f.i8("heat", &mut self.heat, Quantize::Exact);
            f.quat("aim", &mut self.aim, Quantize::Bits(9));
        }
    }
    // 8 bits of heat; a rotation as its three smallest parts plus which was dropped.
    assert_eq!(replicated_bits::<Turret>(), 8 + 29);
}

/// Declared replication refuses misuse with a message that names the fix.
mod declared_replication_misuse {

    use ponko::net::{InterestConfig, PeerId, Quantize, RawField, RawType, Replicate, Replication, SnapshotConfig};
    use ponko::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
    struct Health(#[replicate(bits = 7)] u8);

    #[derive(Replicate, Clone, Copy, Default)]
    struct Stick(#[replicate(min = -1.0, max = 1.0, step = 0.01)] f32);

    #[derive(Replicate, Clone, Copy, Default)]
    struct Pedals(bool);

    #[derive(Serialize, Deserialize)]
    struct Hello;

    /// An event whose `Serialize` refuses it: it can't be sent.
    #[derive(Deserialize)]
    struct Grid;

    impl Serialize for Grid {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("a grid stays on the server"))
        }
    }

    #[derive(Component, Clone, Copy)]
    struct Blob(#[allow(dead_code)] u32);

    fn blob_id() -> ponko::ecs_core::ComponentId {
        let mut world = World::new();
        world.spawn((Blob(0),));
        world.component_id::<Blob>().unwrap()
    }

    #[test]
    #[should_panic(expected = "`Health` is declared twice; declare each component once")]
    fn a_component_declared_twice_is_refused() {
        let _ = Replication::new().component::<Health>().component::<Health>();
    }

    #[test]
    #[should_panic(expected = "at most 64 component types can be replicated; put fields that change together")]
    fn more_than_64_component_types_are_refused() {
        let id = blob_id();
        let mut r = Replication::new();
        for i in 0..65 {
            let field = RawField::new("v", 0, RawType::U32, Quantize::Exact);
            r = r.raw_component(&format!("blob{i}"), id, 4, vec![field]);
        }
    }

    #[test]
    #[should_panic(expected = "lies outside its 4 bytes")]
    fn a_raw_field_outside_its_component_is_refused() {
        let field = RawField::new("v", 2, RawType::U32, Quantize::Exact);
        let _ = Replication::new().raw_component("blob", blob_id(), 4, vec![field]);
    }

    #[test]
    #[should_panic(expected = "a game has one input type (put every control in it)")]
    fn a_second_input_type_is_refused() {
        let _ = Replication::new().input::<Stick>().input::<Pedals>();
    }

    #[test]
    #[should_panic(expected = "no input type was declared; add `.input::<Stick>()`")]
    fn an_input_without_a_declared_input_type_is_refused() {
        Replication::new().component::<Health>().set_input(Stick(0.5));
    }

    #[test]
    #[should_panic(expected = "read it with the declared input type")]
    fn reading_an_input_as_another_type_is_refused() {
        let r = Replication::new().input::<Stick>();
        let _ = r.input_of::<Pedals>(PeerId::HOST);
    }

    #[test]
    #[should_panic(expected = "`Hello` isn't a declared event; add `.event::<Hello>()`")]
    fn an_undeclared_event_is_refused() {
        Replication::new().component::<Health>().to_server(&Hello);
    }

    #[test]
    #[should_panic(expected = "event `Hello` is declared twice; declare each event once")]
    fn an_event_declared_twice_is_refused() {
        let _ = Replication::new().event::<Hello>().event::<Hello>();
    }

    #[test]
    #[should_panic(expected = "doesn't serialize")]
    fn an_event_that_does_not_serialize_is_refused() {
        let mut r = Replication::new().event::<Grid>();
        r.to_clients(&Grid);
    }

    #[test]
    #[should_panic(expected = "Replication::interest: InterestConfig: needs 1 to 8 tiers")]
    fn an_invalid_interest_config_is_refused() {
        let _ = Replication::new().interest(InterestConfig {
            tiers: Vec::new(),
            ..InterestConfig::default()
        });
    }

    #[test]
    #[should_panic(expected = "Replication::interpolated_with: SnapshotConfig: capacity must be at least 2")]
    fn an_invalid_interpolation_config_is_refused() {
        let _ = Replication::new().interpolated_with::<ponko::d2::Transform>(SnapshotConfig {
            capacity: 1,
            ..SnapshotConfig::default()
        });
    }
}
