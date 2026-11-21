//! Game sessions through the lobby server's relay: a lockstep session
//! (`Net::host_relay`, `Net::join_relay`) and a message endpoint
//! (`NetMessages::host_relay`, `NetMessages::join_relay`) with the same API
//! as over UDP, through the real server, over in-memory links that delay,
//! reorder and lose datagrams, and over UDP sockets on this machine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use ponko::net::lobby::LobbyClient;
use ponko::net::{Lane, LoopbackLink, MessageEvent, Net, NetMessages, PeerId};
use ponko::prelude::*;
use ponko_lobby::{LobbyLimits, LobbyServer};

const STEP: Duration = Duration::from_millis(5);

/// A lobby server and `players` clients in one room (the first one hosts
/// it), over in-memory links shaped by `link`, set up on a simulated clock.
fn room(players: usize, link: LoopbackLink) -> (LobbyServer, Vec<LobbyClient>) {
    let mut endpoints = NetMessages::loopback_link(players + 1, link);
    let mut server = LobbyServer::new(LobbyLimits::default());
    server.listen(endpoints.remove(0));
    let mut clients: Vec<LobbyClient> = endpoints.into_iter().map(LobbyClient::new).collect();
    clients[0].create_room(u16::try_from(players).unwrap()).unwrap();
    let mut asked = false;
    for _ in 0..4000 {
        server.poll_elapsed(STEP);
        for client in &mut clients {
            client.poll_elapsed(STEP).unwrap();
        }
        if !asked && let Some(code) = clients[0].room().map(|r| r.code.clone()) {
            for client in &mut clients[1..] {
                client.join_room(&code).unwrap();
            }
            asked = true;
        }
        if clients.iter().all(|c| c.room().is_some_and(|r| r.is_full())) {
            for client in &mut clients {
                while client.next_event().is_some() {}
            }
            assert!(clients[0].is_host());
            return (server, clients);
        }
    }
    panic!("the room never filled");
}

/// Polls the server on its own thread, on the wall clock, until stopped.
fn serve(mut server: LobbyServer) -> (Arc<AtomicBool>, thread::JoinHandle<LobbyServer>) {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            server.poll();
            thread::sleep(Duration::from_millis(1));
        }
        server
    });
    (stop, handle)
}

/// Every player's input folded into one number, tick after tick.
struct Sum(u64);

struct Outcome {
    sum: u64,
    desync: bool,
    player: usize,
}

/// Every player sets an input from the tick and its number; every peer
/// folds all of them in, so one input that differs or goes missing
/// changes the sum.
fn fold(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let me = ctx.net.player() as u64;
    ctx.net.set_input(&(tick * 31 + me * 7));
    let mut sum = ctx.world.resource::<Sum>().0;
    for p in 0..ctx.net.players() {
        let input = ctx.net.input::<u64>(p).unwrap_or(u64::MAX);
        sum = sum.wrapping_mul(1_000_003).wrapping_add(input ^ p as u64);
    }
    ctx.world.resource_mut::<Sum>().0 = sum;
}

/// Runs each session in its own game and thread for `ticks`.
fn play(nets: Vec<Net>, ticks: u64) -> Vec<Outcome> {
    let runs: Vec<_> = nets
        .into_iter()
        .map(|net| {
            thread::spawn(move || {
                let mut world = Game::new("relay", 1, 1)
                    .lockstep(net)
                    .sim_system(fold)
                    .sim_system(move |ctx: &mut Ctx| {
                        if ctx.time.sim_tick() + 1 == ticks {
                            let outcome = Outcome {
                                sum: ctx.world.resource::<Sum>().0,
                                desync: ctx.net.desync().is_some(),
                                player: ctx.net.player(),
                            };
                            ctx.world.insert_resource(outcome);
                        }
                    })
                    .run_headless(ticks, |ctx| ctx.world.insert_resource(Sum(0)));
                world.remove_resource::<Outcome>().expect("ran every tick")
            })
        })
        .collect();
    runs.into_iter().map(|r| r.join().unwrap()).collect()
}

/// Lockstep sessions for the room's members: the host's first.
fn relay_sessions(lobbies: Vec<LobbyClient>, delay: u32) -> Vec<Net> {
    let players = lobbies.len();
    let mut lobbies = lobbies.into_iter();
    let host = Net::host_relay(lobbies.next().unwrap(), players).unwrap().input_delay(delay);
    let mut nets = vec![host];
    nets.extend(lobbies.map(|l| Net::join_relay(l).unwrap()));
    nets
}

/// The same game through the relay, over `link`, and over the in-memory
/// loopback: every peer ends with the loopback's sum.
fn lockstep_through_the_relay_matches_loopback(link: LoopbackLink) {
    const TICKS: u64 = 240;
    const DELAY: u32 = 8;
    let reference = play(
        Net::loopback(3).into_iter().map(|n| n.input_delay(DELAY)).collect(),
        TICKS,
    );
    let (server, lobbies) = room(3, link);
    let (stop, handle) = serve(server);
    let outcomes = play(relay_sessions(lobbies, DELAY), TICKS);
    stop.store(true, Ordering::Relaxed);
    let server = handle.join().unwrap();
    let mut players: Vec<usize> = outcomes.iter().map(|o| o.player).collect();
    players.sort_unstable();
    assert_eq!(players, [0, 1, 2], "every player has its own number");
    for o in &outcomes {
        assert!(!o.desync, "no desync");
        assert_eq!(o.sum, reference[0].sum, "player {} has the loopback's inputs", o.player);
    }
    let stats = server.stats();
    assert!(stats.relay_out > TICKS, "the session went through the relay: {stats:?}");
}

#[test]
fn a_lockstep_session_through_the_relay_survives_latency_and_reordering() {
    // 10 to 30 ms on each link, twice per trip (to the server and on).
    let link = LoopbackLink::new()
        .delay(Duration::from_millis(10))
        .jitter(Duration::from_millis(20))
        .seed(5);
    lockstep_through_the_relay_matches_loopback(link);
}

#[test]
fn a_lockstep_session_through_the_relay_recovers_lost_packets() {
    // One datagram in ten lost on every link.
    let link = LoopbackLink::new().loss(0.1).seed(9);
    lockstep_through_the_relay_matches_loopback(link);
}

/// Pumps the server and `nets` on the wall clock until `done`.
fn pump(server: &mut LobbyServer, nets: &mut [NetMessages], mut done: impl FnMut(&mut [NetMessages]) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        server.poll();
        for net in nets.iter_mut() {
            net.poll().unwrap();
        }
        if done(nets) {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("not done in time");
}

#[test]
fn a_message_session_runs_through_the_relay_over_udp_sockets() {
    let endpoint = NetMessages::host("127.0.0.1:0", 8).unwrap();
    let addr = endpoint.local_addr().unwrap();
    let mut server = LobbyServer::new(LobbyLimits::default());
    server.listen(endpoint);
    let mut host = LobbyClient::join(addr).unwrap();
    let mut guest = LobbyClient::join(addr).unwrap();
    host.create_room(2).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut asked = false;
    while !guest.room().is_some_and(|r| r.is_full()) {
        assert!(std::time::Instant::now() < deadline, "the room never filled");
        server.poll();
        host.poll().unwrap();
        guest.poll().unwrap();
        if !asked && let Some(code) = host.room().map(|r| r.code.clone()) {
            guest.join_room(&code).unwrap();
            asked = true;
        }
        thread::sleep(Duration::from_millis(1));
    }

    // The same calls as over UDP from here on: `host` and `join`'s twins.
    let mut nets = vec![
        NetMessages::host_relay(host, 4).unwrap(),
        NetMessages::join_relay(guest).unwrap(),
    ];
    pump(&mut server, &mut nets, |n| n[1].connected_peers() == 1 && n[0].connected_peers() == 1);
    let client = nets[0]
        .events()
        .find_map(|e| match e {
            MessageEvent::Connected(peer) => Some(peer),
            _ => None,
        })
        .expect("the host saw the client connect");
    for i in 0..50u32 {
        nets[1].send(PeerId::HOST, Lane::Reliable, &i.to_le_bytes()).unwrap();
    }
    nets[0].send(client, Lane::Unreliable, b"state").unwrap();
    let (mut up, mut down) = (Vec::new(), Vec::new());
    pump(&mut server, &mut nets, |n| {
        let [h, c] = n else { unreachable!() };
        up.extend(h.events().filter_map(|e| match e {
            MessageEvent::Message { bytes, .. } => Some(u32::from_le_bytes(bytes[..4].try_into().unwrap())),
            _ => None,
        }));
        down.extend(c.events().filter_map(|e| match e {
            MessageEvent::Message { bytes, lane, .. } => Some((lane, bytes)),
            _ => None,
        }));
        up.len() == 50 && !down.is_empty()
    });
    assert_eq!(up, (0..50).collect::<Vec<u32>>());
    assert_eq!(down, [(Lane::Unreliable, b"state".to_vec())]);
    assert!(server.stats().relay_out >= 50, "through the relay");
}

#[test]
fn a_relay_session_ends_when_the_lobby_server_stops() {
    let (mut server, lobbies) = room(2, LoopbackLink::new());
    let mut lobbies = lobbies.into_iter();
    let mut nets = [
        NetMessages::host_relay(lobbies.next().unwrap(), 2).unwrap(),
        NetMessages::join_relay(lobbies.next().unwrap()).unwrap(),
    ];
    for _ in 0..200 {
        server.poll_elapsed(STEP);
        for net in &mut nets {
            net.poll_elapsed(STEP).unwrap();
        }
    }
    assert_eq!(nets[1].connected_peers(), 1);
    server.shutdown();
    let mut error = None;
    for _ in 0..200 {
        server.poll_elapsed(STEP);
        if let Err(e) = nets[1].poll_elapsed(STEP) {
            error = Some(e);
            break;
        }
    }
    let error = error.expect("the client's poll reports the end of its room");
    assert!(error.to_string().contains("lobby"), "{error}");
    let lobby = nets[1].relay_lobby().expect("a relay endpoint");
    assert!(lobby.room().is_none());
}
