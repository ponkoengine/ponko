//! Browser players behind a reverse proxy: the server counts its limits per
//! client address from the proxy's `X-Forwarded-For` header, but only when
//! `LobbyLimits::trusted_proxies` names the proxy. The proxy here is a small
//! TCP forwarder on this machine that adds the header, as Caddy or nginx do.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use ponko::net::NetMessages;
use ponko::net::lobby::{LobbyClient, LobbyError, LobbyEvent};
use ponko_lobby::{LobbyLimits, LobbyServer, TrustedProxy};

/// A reverse proxy on loopback in front of `upstream` that tells it every
/// client is `forwarded_for`; returns where browsers connect.
fn proxy(upstream: SocketAddr, forwarded_for: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for client in listener.incoming() {
            let Ok(client) = client else { break };
            thread::spawn(move || forward(client, upstream, forwarded_for));
        }
    });
    addr
}

/// Copies the request head with the header added, then bytes both ways.
fn forward(mut client: TcpStream, upstream: SocketAddr, forwarded_for: &str) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match client.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    head.truncate(head.len() - 2);
    head.extend_from_slice(format!("X-Forwarded-For: {forwarded_for}\r\n\r\n").as_bytes());
    let Ok(mut server) = TcpStream::connect(upstream) else { return };
    if server.write_all(&head).is_err() {
        return;
    }
    let (Ok(mut to_client), Ok(mut from_server)) = (client.try_clone(), server.try_clone()) else {
        return;
    };
    thread::spawn(move || {
        let _ = std::io::copy(&mut from_server, &mut to_client);
    });
    let _ = std::io::copy(&mut client, &mut server);
}

/// A server whose WebSocket port the proxies forward to.
fn server(limits: LobbyLimits) -> (LobbyServer, SocketAddr) {
    let endpoint = NetMessages::host("127.0.0.1:0", 64)
        .unwrap()
        .accept_web("127.0.0.1:0")
        .unwrap();
    let web = endpoint.web_addr().unwrap();
    let mut server = LobbyServer::new(limits);
    server.listen(endpoint);
    (server, web)
}

fn browser(via: SocketAddr) -> LobbyClient {
    LobbyClient::join_web(&format!("ws://{via}")).unwrap()
}

/// Polls until `done` holds for the events every client saw so far (a
/// hang detector of 30 s: the WebSocket clients connect on threads).
fn pump(server: &mut LobbyServer, clients: &mut [LobbyClient], mut done: impl FnMut(&[Vec<LobbyEvent>]) -> bool) -> Vec<Vec<LobbyEvent>> {
    let mut seen: Vec<Vec<LobbyEvent>> = clients.iter().map(|_| Vec::new()).collect();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        server.poll();
        for (client, seen) in clients.iter_mut().zip(&mut seen) {
            client.poll().unwrap();
            seen.extend(std::iter::from_fn(|| client.next_event()));
        }
        if done(&seen) {
            return seen;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("not done in time: {seen:?}");
}

fn welcomed(events: &[LobbyEvent]) -> bool {
    events.iter().any(|e| matches!(e, LobbyEvent::Connected { .. }))
}

fn refused(events: &[LobbyEvent], error: LobbyError) -> bool {
    events.iter().any(|e| *e == LobbyEvent::Refused(error))
}

fn limits(trusted: &str) -> LobbyLimits {
    let mut limits = LobbyLimits::default();
    limits.trusted_proxies = TrustedProxy::parse_list(trusted).unwrap();
    limits.connections_per_address = 2;
    limits
}

#[test]
fn behind_a_trusted_proxy_connections_count_per_forwarded_client() {
    let (mut server, web) = server(limits("127.0.0.1"));
    let (one, other) = (proxy(web, "203.0.113.7"), proxy(web, "198.51.100.9"));
    // Three browsers that the proxy says are one client, and one more.
    let mut clients = vec![browser(one), browser(one), browser(one), browser(other)];
    let seen = pump(&mut server, &mut clients, |seen| {
        let refusals = seen.iter().filter(|e| refused(e, LobbyError::TooManyConnections)).count();
        refusals == 1 && seen.iter().filter(|e| welcomed(e)).count() == 4
    });
    assert!(
        seen[..3].iter().any(|e| refused(e, LobbyError::TooManyConnections)),
        "one of the three is over the 2 connections of 203.0.113.7"
    );
    assert!(
        !refused(&seen[3], LobbyError::TooManyConnections),
        "198.51.100.9 has budgets of its own"
    );
}

#[test]
fn the_header_is_ignored_unless_its_sender_is_trusted() {
    // The proxy (127.0.0.1) isn't in the list: its connections count as
    // what they are, 127.0.0.1, whatever the header says.
    let (mut server, web) = server(limits("10.0.0.0/8"));
    let (one, other) = (proxy(web, "203.0.113.7"), proxy(web, "198.51.100.9"));
    let mut clients = vec![browser(one), browser(other), browser(other)];
    let seen = pump(&mut server, &mut clients, |seen| {
        seen.iter().any(|e| refused(e, LobbyError::TooManyConnections))
    });
    assert_eq!(seen.iter().filter(|e| refused(e, LobbyError::TooManyConnections)).count(), 1);
}

#[test]
fn without_trusted_proxies_every_browser_connection_counts_on_its_own() {
    let (mut server, web) = server(limits(""));
    let one = proxy(web, "203.0.113.7");
    let mut clients = vec![browser(one), browser(one), browser(one)];
    let seen = pump(&mut server, &mut clients, |seen| seen.iter().all(|e| welcomed(e)));
    assert!(seen.iter().all(|e| !refused(e, LobbyError::TooManyConnections)));
}

#[test]
fn request_rate_limits_use_the_forwarded_client_address() {
    let mut limits = limits("127.0.0.1");
    limits.request_burst = 2.0;
    limits.requests_per_second = 0.01;
    let (mut server, web) = server(limits);
    let (one, other) = (proxy(web, "203.0.113.7"), proxy(web, "198.51.100.9"));
    let mut clients = vec![browser(one), browser(one), browser(other)];
    pump(&mut server, &mut clients, |seen| seen.iter().all(|e| welcomed(e)));
    // Two requests from 203.0.113.7 use its burst; its next one is refused.
    clients[0].create_room(2).unwrap();
    clients[0].leave_room().unwrap();
    pump(&mut server, &mut clients, |seen| {
        seen[0].iter().any(|e| matches!(e, LobbyEvent::Left(_)))
    });
    clients[1].create_room(2).unwrap();
    clients[2].create_room(2).unwrap();
    let seen = pump(&mut server, &mut clients, |seen| {
        refused(&seen[1], LobbyError::RateLimited) && seen[2].iter().any(|e| matches!(e, LobbyEvent::Joined(_)))
    });
    assert!(refused(&seen[1], LobbyError::RateLimited), "same client address, shared budget");
    assert!(!refused(&seen[2], LobbyError::RateLimited), "another client address, its own budget");
}
