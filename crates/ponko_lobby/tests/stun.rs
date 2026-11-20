//! The STUN responder (`LOBBY_STUN`): binding requests are answered with
//! the address they came from, other datagrams ignored, and answers to one
//! IP address limited. Real UDP sockets on loopback.

use std::net::UdpSocket;
use std::time::Duration;

use ponko::net::stun;
use ponko_lobby::StunResponder;

fn client() -> UdpSocket {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    socket
}

/// Polls until the responder has seen `n` datagrams in all.
fn poll_until_seen(responder: &mut StunResponder, n: u64) {
    for _ in 0..100_000 {
        responder.poll();
        let s = responder.stats();
        if s.answered + s.rate_limited + s.ignored >= n {
            return;
        }
    }
    panic!("the responder never saw {n} datagrams: {:?}", responder.stats());
}

#[test]
fn a_binding_request_is_answered_with_the_senders_address() {
    let mut responder = StunResponder::bind("127.0.0.1:0").unwrap();
    let socket = client();
    socket
        .send_to(&stun::binding_request([3; 12]), responder.local_addr().unwrap())
        .unwrap();
    poll_until_seen(&mut responder, 1);
    let mut buf = [0u8; 128];
    let (n, from) = socket.recv_from(&mut buf).unwrap();
    assert_eq!(from, responder.local_addr().unwrap());
    assert_eq!(
        stun::mapped_address(&buf[..n], [3; 12]),
        Some(socket.local_addr().unwrap())
    );
    assert_eq!(responder.stats().answered, 1);
}

#[test]
fn other_datagrams_are_ignored_and_answers_per_address_are_limited() {
    let mut responder = StunResponder::bind("127.0.0.1:0").unwrap();
    let to = responder.local_addr().unwrap();
    let socket = client();
    socket.send_to(b"not stun at all, 20+ bytes", to).unwrap();
    poll_until_seen(&mut responder, 1);
    assert_eq!(responder.stats().ignored, 1);

    // A burst of 100 requests from one address: about 40 are answered (the
    // burst, plus what refills while they arrive), the rest refused.
    for i in 0..100u8 {
        socket.send_to(&stun::binding_request([i; 12]), to).unwrap();
    }
    poll_until_seen(&mut responder, 101);
    let s = responder.stats();
    assert!((40..60).contains(&s.answered), "{s:?}");
    assert_eq!(s.answered + s.rate_limited, 100);
}
