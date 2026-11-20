//! The lobby over real sockets on this machine: UDP for desktop players,
//! WebSocket for browsers, both on one server and in one room.

use std::time::Duration;

use ponko::net::lobby::{LobbyClient, LobbyEvent};
use ponko::net::{Lane, NetMessages};
use ponko_lobby::{LobbyLimits, LobbyServer};

const STEP: Duration = Duration::from_millis(5);

fn pump(
    server: &mut LobbyServer,
    clients: &mut [&mut LobbyClient],
    mut done: impl FnMut(&mut [&mut LobbyClient]) -> bool,
) {
    // At most 4 s of wall time: loopback packets arrive at once, the
    // WebSocket client connects on a thread of its own.
    for _ in 0..800 {
        server.poll_elapsed(STEP);
        for client in clients.iter_mut() {
            client.poll_elapsed(STEP).unwrap();
        }
        if done(clients) {
            return;
        }
        std::thread::sleep(STEP);
    }
    panic!("not done in time");
}

#[test]
fn desktop_and_browser_players_share_a_room_over_udp_and_websocket() {
    let endpoint = NetMessages::host("127.0.0.1:0", 8)
        .unwrap()
        .accept_web("127.0.0.1:0")
        .unwrap();
    let (udp, web) = (endpoint.local_addr().unwrap(), endpoint.web_addr().unwrap());
    let mut server = LobbyServer::new(LobbyLimits::default());
    server.listen(endpoint);

    let mut desktop = LobbyClient::join(udp).unwrap();
    let mut browser = LobbyClient::join_web(&format!("ws://{web}")).unwrap();
    desktop.create_room(2).unwrap();
    pump(&mut server, &mut [&mut desktop, &mut browser], |c| {
        c[0].room().is_some() && c[1].is_connected()
    });
    let code = desktop.room().unwrap().code.clone();
    browser.join_room(&code).unwrap();
    pump(&mut server, &mut [&mut desktop, &mut browser], |c| {
        c[0].room().is_some_and(|r| r.is_full())
    });
    while desktop.next_event().is_some() {}
    while browser.next_event().is_some() {}

    browser.relay(None, Lane::Reliable, b"from the browser").unwrap();
    desktop.relay(None, Lane::Reliable, b"from the desktop").unwrap();
    let (mut to_desktop, mut to_browser) = (Vec::new(), Vec::new());
    pump(&mut server, &mut [&mut desktop, &mut browser], |c| {
        let [d, b] = c else { unreachable!() };
        to_desktop.extend(std::iter::from_fn(|| d.next_event()));
        to_browser.extend(std::iter::from_fn(|| b.next_event()));
        !to_desktop.is_empty() && !to_browser.is_empty()
    });
    assert!(matches!(&to_desktop[..], [LobbyEvent::Relayed { bytes, .. }] if bytes == b"from the browser"));
    assert!(matches!(&to_browser[..], [LobbyEvent::Relayed { bytes, .. }] if bytes == b"from the desktop"));
    assert_eq!(server.stats().connections, 2);
}
