use ponko::net::Lane;
use ponko::net::MAX_MESSAGE_BYTES;
use ponko::net::MessageEvent;
use ponko::net::NetMessages;
use ponko::net::PeerId;
use std::time::{Duration, Instant};

/// How long the WebSocket tests wait for something to arrive. A browser
/// client connects on a thread of its own, so a number of rounds would be a
/// time limit in disguise; this is a hang detector, and each wait ends as
/// soon as what it waits for is in.
const WEB_WAIT: Duration = Duration::from_secs(30);

fn step(nets: &mut [NetMessages], rounds: usize) {
    for _ in 0..rounds {
        for net in nets.iter_mut() {
            net.poll_elapsed(Duration::from_millis(20)).unwrap();
        }
    }
}
/// Like [`step`], for real sockets: waits a little each round so packets
/// arrive even on a busy machine.
fn step_udp(nets: &mut [NetMessages], rounds: usize) {
    for _ in 0..rounds {
        std::thread::sleep(Duration::from_millis(2));
        step(nets, 1);
    }
}
fn connected(nets: &mut [NetMessages]) -> PeerId {
    step(nets, 40);
    let peer = nets[0]
        .events()
        .find_map(|e| match e {
            MessageEvent::Connected(p) => Some(p),
            _ => None,
        })
        .expect("host connected");
    assert!(
        nets[1]
            .events()
            .any(|e| matches!(e, MessageEvent::Connected(PeerId::HOST)))
    );
    peer
}

#[test]
fn reliable_survives_loss_and_preserves_order() {
    let mut nets = NetMessages::loopback_lossy(2, 4);
    let peer = connected(&mut nets);
    for i in 0u32..200 {
        nets[0].send(peer, Lane::Reliable, &i.to_le_bytes()).unwrap();
    }
    step(&mut nets, 300);
    let messages: Vec<_> = nets[1]
        .events()
        .filter_map(|e| match e {
            MessageEvent::Message {
                lane: Lane::Reliable,
                bytes,
                ..
            } => Some(u32::from_le_bytes(bytes.try_into().unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(messages, (0..200).collect::<Vec<_>>());
}

#[test]
fn fragmented_large_message_is_reassembled_under_loss() {
    let mut nets = NetMessages::loopback_lossy(2, 5);
    let peer = connected(&mut nets);
    let bytes: Vec<_> = (0..100_000).map(|i| (i % 251) as u8).collect();
    nets[0].send(peer, Lane::Reliable, &bytes).unwrap();
    step(&mut nets, 400);
    let messages: Vec<_> = nets[1]
        .events()
        .filter_map(|e| match e {
            MessageEvent::Message { bytes, .. } => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec![bytes]);
}

#[test]
fn unreliable_drops_stale_messages_when_packets_are_reversed() {
    // In-memory transport deliberately receives newest packets first.
    let mut nets = NetMessages::loopback(2);
    let peer = connected(&mut nets);
    for i in 0u32..8 {
        nets[0].send(peer, Lane::Unreliable, &i.to_le_bytes()).unwrap();
        nets[0].poll_elapsed(Duration::from_millis(20)).unwrap();
    }
    nets[1].poll_elapsed(Duration::from_millis(20)).unwrap();
    let messages: Vec<_> = nets[1]
        .events()
        .filter_map(|e| match e {
            MessageEvent::Message {
                lane: Lane::Unreliable,
                bytes,
                ..
            } => Some(u32::from_le_bytes(bytes.try_into().unwrap())),
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec![7]);
}

#[test]
fn disconnect_and_silent_timeout_are_reported_once() {
    let mut nets = NetMessages::loopback(2);
    let peer = connected(&mut nets);
    nets[0].disconnect(peer).unwrap();
    step(&mut nets, 5);
    assert_eq!(
        nets[1]
            .events()
            .filter(|e| matches!(e, MessageEvent::Disconnected(..)))
            .count(),
        1
    );
    let mut nets = NetMessages::loopback(2);
    connected(&mut nets);
    for _ in 0..10 {
        nets[1].poll_elapsed(Duration::from_secs(1)).unwrap();
    }
    assert_eq!(
        nets[1].next_event(),
        Some(MessageEvent::Disconnected(PeerId::HOST, "peer timed out".into()))
    );
    nets[1].poll_elapsed(Duration::from_secs(1)).unwrap();
    assert_eq!(nets[1].next_event(), None);
}

#[test]
fn udp_connects_and_moves_bytes_both_ways() {
    let host = NetMessages::host("127.0.0.1:0", 1).unwrap();
    let client = NetMessages::join(host.local_addr().unwrap()).unwrap();
    let mut nets = vec![host, client];
    step_udp(&mut nets, 100);
    let peer = connected(&mut nets);
    nets[0].send(peer, Lane::Reliable, b"hello").unwrap();
    nets[1].send(PeerId::HOST, Lane::Unreliable, b"reply").unwrap();
    step_udp(&mut nets, 50);
    assert!(
        nets[1]
            .events()
            .any(|e| matches!(e, MessageEvent::Message { bytes, .. } if bytes == b"hello"))
    );
    assert!(
        nets[0]
            .events()
            .any(|e| matches!(e, MessageEvent::Message { bytes, .. } if bytes == b"reply"))
    );
}

#[test]
fn udp_and_web_clients_join_one_host_on_every_lane() {
    let host = NetMessages::host("127.0.0.1:0", 2)
        .unwrap()
        .accept_web("127.0.0.1:0")
        .unwrap();
    let web_addr = host.web_addr().expect("listening for browsers");
    assert_eq!(host.local_addr().map(|a| a.ip()), Some(web_addr.ip()));
    let udp = NetMessages::join(host.local_addr().unwrap()).unwrap();
    let web = NetMessages::join_web(&format!("ws://{web_addr}")).unwrap();
    assert_eq!(web.local_addr(), None);
    let mut nets = vec![host, udp, web];
    let mut peers = Vec::new();
    let mut joined = [false; 2];
    let deadline = Instant::now() + WEB_WAIT;
    while Instant::now() < deadline {
        step_udp(&mut nets, 1);
        peers.extend(nets[0].events().filter_map(|e| match e {
            MessageEvent::Connected(p) => Some(p),
            _ => None,
        }));
        for (client, joined) in joined.iter_mut().enumerate() {
            *joined |= nets[client + 1]
                .events()
                .any(|e| e == MessageEvent::Connected(PeerId::HOST));
        }
        if peers.len() == 2 && joined == [true; 2] {
            break;
        }
    }
    assert_eq!(peers.len(), 2, "the host saw both clients");
    assert_eq!(joined, [true; 2], "both clients saw the host");
    nets[0].send_to_all(Lane::Reliable, b"to everyone").unwrap();
    for (i, lane) in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable]
        .into_iter()
        .enumerate()
    {
        for (client, net) in nets.iter_mut().enumerate().skip(1) {
            net.send(PeerId::HOST, lane, &[client as u8, i as u8]).unwrap();
        }
    }
    let mut heard = Vec::new();
    let mut everyone = [false; 2];
    let deadline = Instant::now() + WEB_WAIT;
    while Instant::now() < deadline {
        step_udp(&mut nets, 1);
        heard.extend(nets[0].events().filter_map(|e| match e {
            MessageEvent::Message { bytes, lane, .. } => Some((bytes, lane)),
            _ => None,
        }));
        for (client, got) in everyone.iter_mut().enumerate() {
            *got |= nets[client + 1]
                .events()
                .any(|e| matches!(e, MessageEvent::Message { bytes, .. } if bytes == b"to everyone"));
        }
        if heard.len() == 6 && everyone == [true; 2] {
            break;
        }
    }
    let lanes = [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable];
    let mut got: Vec<Vec<u8>> = heard
        .iter()
        .map(|(bytes, lane)| {
            assert_eq!(
                *lane,
                lanes[usize::from(bytes[1])],
                "arrived on the lane it was sent on"
            );
            bytes.clone()
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        [[1, 0], [1, 1], [1, 2], [2, 0], [2, 1], [2, 2]].map(Vec::from),
        "every lane from both clients"
    );
    assert_eq!(everyone, [true; 2], "the broadcast reached the UDP and the web client");
}

#[test]
fn web_endpoints_check_their_role_and_url() {
    let client = NetMessages::join("127.0.0.1:7777").unwrap();
    assert_eq!(
        client.accept_web("127.0.0.1:0").err().map(|e| e.kind()),
        Some(std::io::ErrorKind::InvalidInput)
    );
    let host = NetMessages::host("127.0.0.1:0", 1)
        .unwrap()
        .accept_web("127.0.0.1:0")
        .unwrap();
    assert!(host.accept_web("127.0.0.1:0").is_err(), "one listener per host");
    assert!(NetMessages::join_web("udp://127.0.0.1:1").is_err());
}

#[test]
fn a_closed_web_host_disconnects_its_client() {
    let host = NetMessages::host("127.0.0.1:0", 1)
        .unwrap()
        .accept_web("127.0.0.1:0")
        .unwrap();
    let web = NetMessages::join_web(&format!("ws://{}", host.web_addr().unwrap())).unwrap();
    let mut nets = vec![host, web];
    let deadline = Instant::now() + WEB_WAIT;
    while Instant::now() < deadline {
        step_udp(&mut nets, 1);
        if nets[1].events().any(|e| e == MessageEvent::Connected(PeerId::HOST)) {
            break;
        }
    }
    drop(nets.remove(0));
    let mut error = None;
    let deadline = Instant::now() + WEB_WAIT;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
        if let Err(e) = nets[0].poll_elapsed(Duration::from_millis(20)) {
            error = Some(e);
            break;
        }
    }
    let error = error.expect("the closed connection is reported");
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionAborted, "{error}");
}

#[test]
fn oversize_and_unknown_peer_are_errors_and_serde_round_trips() {
    let mut nets = NetMessages::loopback(2);
    let peer = connected(&mut nets);
    assert!(
        nets[0]
            .send(peer, Lane::Reliable, &vec![0; MAX_MESSAGE_BYTES + 1])
            .is_err()
    );
    assert!(nets[0].send(PeerId::HOST, Lane::Reliable, b"bad").is_err());
    nets[0].send_serde(peer, Lane::Reliable, &vec![1u32, 2, 3]).unwrap();
    step(&mut nets, 10);
    assert!(nets[1].events().any(|e| e.decode::<Vec<u32>>() == Some(vec![1, 2, 3])));
}

#[test]
fn full_host_refuses_extra_clients_and_broadcast_reaches_every_client() {
    let host = NetMessages::host("127.0.0.1:0", 1).unwrap();
    let addr = host.local_addr().unwrap();
    let mut nets = vec![host, NetMessages::join(addr).unwrap()];
    step_udp(&mut nets, 100);
    connected(&mut nets);
    nets.push(NetMessages::join(addr).unwrap());
    step_udp(&mut nets, 100);
    assert!(
        nets[2]
            .events()
            .any(|e| matches!(e, MessageEvent::Disconnected(_, reason) if reason.contains("full")))
    );

    let mut nets = NetMessages::loopback(4);
    step(&mut nets, 40);
    for net in &mut nets {
        net.events().for_each(drop);
    }
    nets[0].send_to_all(Lane::Reliable, b"broadcast").unwrap();
    step(&mut nets, 20);
    for net in &mut nets[1..] {
        assert!(
            net.events()
                .any(|e| matches!(e, MessageEvent::Message { bytes, .. } if bytes == b"broadcast"))
        );
    }
}

#[test]
fn silent_join_times_out_after_its_first_poll() {
    let mut client = NetMessages::loopback_lossy(2, 1).remove(1);
    client.poll_elapsed(Duration::from_secs(30)).unwrap();
    for _ in 0..9 {
        client.poll_elapsed(Duration::from_secs(1)).unwrap();
        assert_eq!(client.next_event(), None);
    }
    client.poll_elapsed(Duration::from_secs(1)).unwrap();
    assert_eq!(
        client.next_event(),
        Some(MessageEvent::Disconnected(PeerId::HOST, "peer timed out".into()))
    );
    client.poll_elapsed(Duration::from_secs(1)).unwrap();
    assert_eq!(client.next_event(), None);
}

#[test]
fn full_send_queue_returns_backpressure() {
    let mut nets = NetMessages::loopback(2);
    let peer = connected(&mut nets);
    let payload = vec![1; MAX_MESSAGE_BYTES];
    for _ in 0..4 {
        nets[0].send(peer, Lane::Reliable, &payload).unwrap();
    }
    assert_eq!(
        nets[0].send(peer, Lane::Reliable, b"full").unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn unreliable_fragments_and_empty_reliable_messages_round_trip() {
    let mut nets = NetMessages::loopback(2);
    let peer = connected(&mut nets);
    nets[0].send(peer, Lane::Reliable, b"").unwrap();
    // Large unreliable messages must not be silently discarded by Renet
    // merely because they exceed its default 60 KB update budget.
    let payload = vec![7; 100_000];
    nets[0].send(peer, Lane::Unreliable, &payload).unwrap();
    step(&mut nets, 20);
    let messages: Vec<_> = nets[1]
        .events()
        .filter_map(|e| match e {
            MessageEvent::Message { lane, bytes, .. } => Some((lane, bytes)),
            _ => None,
        })
        .collect();
    assert!(messages.contains(&(Lane::Reliable, vec![])));
    assert!(messages.contains(&(Lane::Unreliable, payload)));
}

#[test]
fn reliable_unordered_survives_loss_without_duplicates() {
    let mut nets = NetMessages::loopback_lossy(2, 4);
    let peer = connected(&mut nets);
    for i in 0u32..200 {
        nets[0].send(peer, Lane::ReliableUnordered, &i.to_le_bytes()).unwrap();
    }
    step(&mut nets, 300);
    let mut got: Vec<_> = nets[1]
        .events()
        .filter_map(|e| match e {
            MessageEvent::Message {
                lane: Lane::ReliableUnordered,
                bytes,
                ..
            } => Some(u32::from_le_bytes(bytes.try_into().unwrap())),
            _ => None,
        })
        .collect();
    got.sort_unstable();
    assert_eq!(got, (0..200).collect::<Vec<_>>());
}
