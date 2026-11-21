//! Scripted tests of the WebRTC transport (`rtc.rs`, `rtc_native.rs`): real
//! str0m connections (ICE, DTLS, SCTP) over an in-memory network, on the
//! simulated clock of `NetMessages::poll_elapsed`, with an in-memory
//! signaling channel. No sockets (but for one loopback test), no internet,
//! no sleeps.
//!
//! The network can put an endpoint behind a NAT: a port-restricted cone
//! NAT (one public port per socket, whatever the destination; only peers
//! the socket sent to may answer: most home routers) or a symmetric one (a
//! new public port per destination: some mobile and corporate networks). A
//! STUN server on the network answers binding requests with the address it
//! sees, as a public one does, and a TURN server (`turn.rs`'s test server)
//! relays for peers that allocated an address on it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::rtc::{Agent, Signaling};
use super::rtc_native::Socket;
use super::stun;
use super::turn::test_server::{Out, TestTurnServer};
use crate::net::{Lane, MessageEvent, NetMessages, PeerId};

/// One scripted step: every endpoint polls with this much protocol time.
const STEP: Duration = Duration::from_millis(10);
const STUN_SERVER: &str = "198.51.100.1:3478";
const TURN_SERVER: &str = "198.51.100.2:3478";

#[derive(Clone, Copy, PartialEq, Eq)]
enum NatKind {
    PortRestrictedCone,
    Symmetric,
}

struct Nat {
    public: IpAddr,
    kind: NatKind,
    /// (private source, destination for a symmetric NAT) -> public port.
    out: BTreeMap<(SocketAddr, Option<SocketAddr>), u16>,
    /// Public port -> private address.
    back: BTreeMap<u16, SocketAddr>,
    /// (public port, remote) pairs allowed in: remotes the socket sent to.
    allowed: BTreeSet<(u16, SocketAddr)>,
    next_port: u16,
}

#[derive(Default)]
struct Network {
    inboxes: BTreeMap<SocketAddr, VecDeque<(SocketAddr, Vec<u8>)>>,
    /// Private IP -> its NAT.
    nats: BTreeMap<IpAddr, Nat>,
    /// Drop every nth datagram between peers (STUN to the server is kept).
    drop_every: u64,
    sent: u64,
    /// Datagrams delivered from one public address to another, by kind.
    delivered: Vec<(SocketAddr, SocketAddr)>,
    /// Every destination peers sent to, delivered or not.
    tried: Vec<SocketAddr>,
    stun_requests: u64,
    /// The TURN server at `TURN_SERVER`, if the test runs one.
    turn: Option<TestTurnServer>,
    /// The password agents give the TURN server (`secret` is right).
    turn_password: String,
}

impl Network {
    fn nat(&mut self, private_ip: &str, public_ip: &str, kind: NatKind) {
        self.nats.insert(
            private_ip.parse().unwrap(),
            Nat {
                public: public_ip.parse().unwrap(),
                kind,
                out: BTreeMap::new(),
                back: BTreeMap::new(),
                allowed: BTreeSet::new(),
                next_port: 40000,
            },
        );
    }

    /// The address `from`'s datagram to `to` comes from, as the world sees
    /// it, after its NAT (if any) maps it and lets answers from `to` in.
    fn outward(&mut self, from: SocketAddr, to: SocketAddr) -> SocketAddr {
        let Some(nat) = self.nats.get_mut(&from.ip()) else {
            return from;
        };
        let key = (from, (nat.kind == NatKind::Symmetric).then_some(to));
        let port = match nat.out.get(&key) {
            Some(&port) => port,
            None => {
                let port = nat.next_port;
                nat.next_port += 1;
                nat.out.insert(key, port);
                nat.back.insert(port, from);
                port
            }
        };
        nat.allowed.insert((port, to));
        SocketAddr::new(nat.public, port)
    }

    /// Where a datagram from `seen` to `to` lands, if anywhere.
    fn inward(&self, seen: SocketAddr, to: SocketAddr) -> Option<SocketAddr> {
        if let Some(nat) = self.nats.values().find(|n| n.public == to.ip()) {
            let private = *nat.back.get(&to.port())?;
            return nat.allowed.contains(&(to.port(), seen)).then_some(private);
        }
        // A private address behind a NAT can't be reached from outside.
        if self.nats.contains_key(&to.ip()) && !self.nats.contains_key(&seen.ip()) {
            return None;
        }
        self.inboxes.contains_key(&to).then_some(to)
    }

    fn run_turn(&mut self) {
        let mut next = 50_000;
        self.turn = Some(TestTurnServer::new("alice", "secret", move || {
            next += 1;
            SocketAddr::new("198.51.100.2".parse().unwrap(), next)
        }));
    }

    /// A datagram for the TURN server, at its listening address or at one
    /// of its relayed ones; false if `to` is neither.
    fn to_turn(&mut self, seen: SocketAddr, to: SocketAddr, bytes: &[u8]) -> bool {
        let Some(mut turn) = self.turn.take() else {
            return false;
        };
        let server: SocketAddr = TURN_SERVER.parse().unwrap();
        let mut out = Vec::new();
        let handled = if to == server {
            turn.from_client(seen, bytes, &mut out);
            true
        } else if turn.relays().any(|r| r == to) {
            if let Some((client, data)) = turn.from_peer(to, seen, bytes) {
                out.push(Out::ToClient(client, data));
            }
            true
        } else {
            false
        };
        self.turn = Some(turn);
        for o in out {
            match o {
                Out::ToClient(client, data) => {
                    if let Some(at) = self.inward(server, client) {
                        self.inboxes.get_mut(&at).unwrap().push_back((server, data));
                    }
                }
                Out::FromRelay { relay, to, bytes } => self.send(relay, to, &bytes),
            }
        }
        handled
    }

    fn send(&mut self, from: SocketAddr, to: SocketAddr, bytes: &[u8]) {
        let seen = self.outward(from, to);
        if self.to_turn(seen, to, bytes) {
            return;
        }
        if to == STUN_SERVER.parse().unwrap() {
            self.stun_requests += 1;
            let mut reply = Vec::new();
            if stun::binding_response(bytes, seen, &mut reply) {
                let server = to;
                // The answer comes back through the NAT: allowed, as the
                // request went out to the server.
                if let Some(at) = self.inward(server, seen) {
                    self.inboxes.get_mut(&at).unwrap().push_back((server, reply));
                }
            }
            return;
        }
        self.sent += 1;
        self.tried.push(to);
        if self.drop_every != 0 && self.sent.is_multiple_of(self.drop_every) {
            return;
        }
        if let Some(at) = self.inward(seen, to) {
            self.delivered.push((seen, to));
            self.inboxes.get_mut(&at).unwrap().push_back((seen, bytes.to_vec()));
        }
    }
}

type Shared = Arc<Mutex<Network>>;

struct MemSocket {
    net: Shared,
    me: SocketAddr,
}

impl Socket for MemSocket {
    fn send_to(&mut self, bytes: &[u8], to: SocketAddr) -> io::Result<()> {
        self.net.lock().unwrap().send(self.me, to, bytes);
        Ok(())
    }
    fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>> {
        let mut net = self.net.lock().unwrap();
        Ok(net.inboxes.get_mut(&self.me).unwrap().pop_front().map(|(from, bytes)| {
            buf[..bytes.len()].copy_from_slice(&bytes);
            (bytes.len(), from)
        }))
    }
}

fn agent(net: &Shared, addr: &str, stun: bool) -> Agent {
    let me: SocketAddr = addr.parse().unwrap();
    net.lock().unwrap().inboxes.insert(me, VecDeque::new());
    let servers: Vec<SocketAddr> = if stun {
        vec![STUN_SERVER.parse().unwrap()]
    } else {
        Vec::new()
    };
    let agent = Agent::with_socket(Box::new(MemSocket { net: net.clone(), me }), me, &servers).unwrap();
    // Agents on a network with a TURN server use it, with the password `password`.
    let turn = net.lock().unwrap().turn.is_some();
    let password = net.lock().unwrap().turn_password.clone();
    if turn {
        agent.with_turn(&[(TURN_SERVER.parse().unwrap(), "alice".into(), password)], false)
    } else {
        agent
    }
}

/// Endpoints with their signaling handles (endpoint `i` is signaling peer
/// `i + 1`), and every event each one saw.
struct Session {
    net: Shared,
    nets: Vec<Option<NetMessages>>,
    signaling: Vec<Signaling>,
    logs: Vec<Vec<MessageEvent>>,
    /// Signaling messages carried, for assertions.
    signals: usize,
}

impl Session {
    /// A host at `addrs[0]` and clients at the others.
    fn new(net: Shared, addrs: &[&str], stun: bool, max_clients: usize) -> Self {
        let signaling: Vec<Signaling> = addrs.iter().map(|_| Signaling::new()).collect();
        let mut nets = vec![Some(NetMessages::rtc_host_with(
            signaling[0].clone(),
            agent(&net, addrs[0], stun),
            max_clients,
        ))];
        for (i, addr) in addrs.iter().enumerate().skip(1) {
            nets.push(Some(NetMessages::rtc_join_with(
                signaling[i].clone(),
                agent(&net, addr, stun),
                1,
            )));
        }
        let logs = addrs.iter().map(|_| Vec::new()).collect();
        Self {
            net,
            nets,
            signaling,
            logs,
            signals: 0,
        }
    }

    fn carry_signals(&mut self) {
        for i in 0..self.signaling.len() {
            while let Some((to, bytes)) = self.signaling[i].next_outgoing() {
                self.signals += 1;
                let to = usize::try_from(to).unwrap() - 1;
                self.signaling[to].deliver(i as u64 + 1, &bytes);
            }
        }
    }

    fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            for i in 0..self.nets.len() {
                if let Some(net) = &mut self.nets[i] {
                    net.poll_elapsed(STEP).unwrap();
                    self.logs[i].extend(net.events());
                }
            }
            self.carry_signals();
        }
    }

    fn connected(&self, i: usize) -> usize {
        self.logs[i]
            .iter()
            .filter(|e| matches!(e, MessageEvent::Connected(_)))
            .count()
    }

    /// Steps until every client and the host saw `Connected`; the steps it took.
    fn connect(&mut self, max_steps: usize) -> Option<usize> {
        let clients = self.nets.len() - 1;
        for steps in 0..max_steps {
            if self.connected(0) == clients && (1..=clients).all(|i| self.connected(i) == 1) {
                return Some(steps);
            }
            self.step(1);
        }
        None
    }

    fn net(&mut self, i: usize) -> &mut NetMessages {
        self.nets[i].as_mut().unwrap()
    }

    fn host_peer_of(&self, client: usize) -> PeerId {
        // Host peers are numbered in handshake order; find the one whose
        // signaling peer is this client.
        let host = self.nets[0].as_ref().unwrap();
        self.logs[0]
            .iter()
            .find_map(|e| match e {
                MessageEvent::Connected(p) if host.rtc_signal_peer(*p) == Some(client as u64 + 1) => Some(*p),
                _ => None,
            })
            .unwrap()
    }

    fn messages(&mut self, i: usize, lane: Lane) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        self.logs[i].retain(|e| match e {
            MessageEvent::Message { lane: l, bytes, .. } if *l == lane => {
                out.push(bytes.clone());
                false
            }
            _ => true,
        });
        out
    }
}

fn open_network() -> Shared {
    Arc::new(Mutex::new(Network::default()))
}

#[test]
fn a_client_joins_a_host_and_every_lane_delivers_both_ways() {
    let mut s = Session::new(open_network(), &["203.0.113.1:5000", "203.0.113.2:5000"], false, 4);
    let steps = s.connect(500).expect("connects");
    assert!(steps < 200, "took {steps} steps of 10 ms");
    assert_eq!(s.net(1).rtc_signal_peer(PeerId::HOST), Some(1));
    let peer = s.host_peer_of(1);
    for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
        s.net(1).send(PeerId::HOST, lane, b"up").unwrap();
        s.net(0).send(peer, lane, b"down").unwrap();
    }
    s.step(20);
    for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
        assert_eq!(s.messages(0, lane), [b"up".to_vec()], "{lane:?} to the host");
        assert_eq!(s.messages(1, lane), [b"down".to_vec()], "{lane:?} to the client");
    }
    // The packets went straight between the peers, not through signaling:
    // a handful of signals set the connection up, nothing more.
    assert!(s.signals <= 12, "{} signals", s.signals);
}

#[test]
fn reliable_lanes_survive_loss_and_unreliable_messages_are_never_stale() {
    let net = open_network();
    let mut s = Session::new(net.clone(), &["203.0.113.1:5000", "203.0.113.2:5000"], false, 4);
    s.connect(500).expect("connects");
    net.lock().unwrap().drop_every = 5;
    for i in 0..100u32 {
        s.net(1).send(PeerId::HOST, Lane::Reliable, &i.to_le_bytes()).unwrap();
        s.net(1).send(PeerId::HOST, Lane::Unreliable, &i.to_le_bytes()).unwrap();
        s.step(1);
    }
    s.step(100);
    let number = |b: &Vec<u8>| u32::from_le_bytes(b.as_slice().try_into().unwrap());
    let reliable: Vec<u32> = s.messages(0, Lane::Reliable).iter().map(number).collect();
    assert_eq!(reliable, (0..100).collect::<Vec<_>>(), "every one, in order");
    let unreliable: Vec<u32> = s.messages(0, Lane::Unreliable).iter().map(number).collect();
    assert!(unreliable.windows(2).all(|w| w[0] < w[1]), "never stale or repeated");
    assert!(unreliable.len() < 100, "some were lost, and not resent");
}

#[test]
fn peers_behind_two_cone_nats_connect_with_stun_and_not_without() {
    for stun in [true, false] {
        let net = open_network();
        {
            let mut n = net.lock().unwrap();
            n.nat("10.0.0.2", "198.51.100.20", NatKind::PortRestrictedCone);
            n.nat("10.1.0.2", "198.51.100.30", NatKind::PortRestrictedCone);
        }
        let mut s = Session::new(net.clone(), &["10.0.0.2:5000", "10.1.0.2:5000"], stun, 4);
        let connected = s.connect(1500);
        if stun {
            connected.expect("connects through both NATs");
            let n = net.lock().unwrap();
            assert!(n.stun_requests >= 2, "both asked the STUN server");
            // Packets crossed between the two public (mapped) addresses: a
            // hole punched in each NAT.
            assert!(n.delivered.iter().any(|(from, to)| {
                from.ip().to_string() == "198.51.100.30" && to.ip().to_string() == "198.51.100.20"
            }));
        } else {
            assert_eq!(connected, None, "without STUN neither knows the other's public address");
        }
    }
}

#[test]
fn a_peer_behind_a_symmetric_nat_reaches_a_public_host_but_two_symmetric_nats_need_turn() {
    // One public host: the client behind a symmetric NAT connects (the host
    // learns its address from its checks: a peer-reflexive candidate).
    let net = open_network();
    net.lock().unwrap().nat("10.1.0.2", "198.51.100.30", NatKind::Symmetric);
    let mut s = Session::new(net, &["203.0.113.1:5000", "10.1.0.2:5000"], true, 4);
    s.connect(1500).expect("a symmetric NAT reaches a public host");

    // Both behind symmetric NATs: no address either learns works for the
    // other. Without a TURN server they can't connect (the next test has one).
    let net = open_network();
    {
        let mut n = net.lock().unwrap();
        n.nat("10.0.0.2", "198.51.100.20", NatKind::Symmetric);
        n.nat("10.1.0.2", "198.51.100.30", NatKind::Symmetric);
    }
    let mut s = Session::new(net, &["10.0.0.2:5000", "10.1.0.2:5000"], true, 4);
    assert_eq!(s.connect(1500), None);
    // The client's endpoint reports it as any failed join: a timeout.
    s.step(1000);
    assert!(
        s.logs[1]
            .iter()
            .any(|e| matches!(e, MessageEvent::Disconnected(PeerId::HOST, _)))
    );
}

/// Two peers behind symmetric NATs, with STUN and a TURN server on the
/// network that agents reach with `password`.
fn behind_symmetric_nats_with_turn(password: &str) -> (Shared, Session) {
    let net = open_network();
    {
        let mut n = net.lock().unwrap();
        n.nat("10.0.0.2", "198.51.100.20", NatKind::Symmetric);
        n.nat("10.1.0.2", "198.51.100.30", NatKind::Symmetric);
        n.run_turn();
        n.turn_password = password.into();
    }
    let s = Session::new(net.clone(), &["10.0.0.2:5000", "10.1.0.2:5000"], true, 4);
    (net, s)
}

#[test]
fn peers_behind_two_symmetric_nats_connect_through_a_turn_server() {
    let (net, mut s) = behind_symmetric_nats_with_turn("secret");
    s.connect(3000).expect("connects through the TURN server");
    let peer = s.host_peer_of(1);
    s.net(1).send(PeerId::HOST, Lane::Reliable, b"up").unwrap();
    s.net(0).send(peer, Lane::Reliable, b"down").unwrap();
    s.step(50);
    assert_eq!(s.messages(0, Lane::Reliable), [b"up".to_vec()]);
    assert_eq!(s.messages(1, Lane::Reliable), [b"down".to_vec()]);
    let n = net.lock().unwrap();
    let turn = n.turn.as_ref().unwrap();
    assert!(
        turn.relayed_to_peers > 0 && turn.relayed_to_clients > 0,
        "the packets went through the relay: {} out, {} in",
        turn.relayed_to_peers,
        turn.relayed_to_clients
    );
    assert_eq!(turn.relays().count(), 2, "each peer allocated a relayed address");
}

#[test]
fn wrong_turn_credentials_leave_two_symmetric_nats_unconnected() {
    let (net, mut s) = behind_symmetric_nats_with_turn("wrong");
    assert_eq!(s.connect(1500), None);
    let n = net.lock().unwrap();
    let turn = n.turn.as_ref().unwrap();
    assert_eq!(turn.relays().count(), 0, "no allocation without the right password");
    assert!(turn.refused >= 2, "both were refused");
}

#[test]
fn a_direct_path_is_preferred_over_the_turn_relay() {
    let net = open_network();
    net.lock().unwrap().run_turn();
    net.lock().unwrap().turn_password = "secret".into();
    let mut s = Session::new(net.clone(), &["203.0.113.1:5000", "203.0.113.2:5000"], false, 4);
    s.connect(1500).expect("connects");
    s.step(200);
    let before = net.lock().unwrap().turn.as_ref().unwrap().relayed_to_peers;
    for i in 0..100u32 {
        s.net(1).send(PeerId::HOST, Lane::Reliable, &i.to_le_bytes()).unwrap();
        s.step(1);
    }
    s.step(50);
    assert_eq!(s.messages(0, Lane::Reliable).len(), 100);
    let after = net.lock().unwrap().turn.as_ref().unwrap().relayed_to_peers;
    assert!(
        after - before < 20,
        "the messages went directly, not through the relay ({} relayed)",
        after - before
    );
}

#[test]
fn a_relay_only_client_joins_through_a_turn_server_on_real_udp_sockets() {
    use super::rtc::RtcConfig;
    use std::net::UdpSocket;

    // The TURN server on loopback: its listening socket and one socket per
    // relayed address, polled with the endpoints.
    let listen = UdpSocket::bind("127.0.0.1:0").unwrap();
    listen.set_nonblocking(true).unwrap();
    let new_relays: Arc<Mutex<Vec<UdpSocket>>> = Arc::default();
    let made = new_relays.clone();
    let mut turn = TestTurnServer::new("alice", "secret", move || {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let addr = socket.local_addr().unwrap();
        made.lock().unwrap().push(socket);
        addr
    });
    let mut relays: BTreeMap<SocketAddr, UdpSocket> = BTreeMap::new();
    let mut poll_turn = |turn: &mut TestTurnServer| {
        let mut buf = [0u8; 4096];
        let mut out = Vec::new();
        while let Ok((n, from)) = listen.recv_from(&mut buf) {
            turn.from_client(from, &buf[..n], &mut out);
        }
        for socket in new_relays.lock().unwrap().drain(..) {
            relays.insert(socket.local_addr().unwrap(), socket);
        }
        for (&relay, socket) in &relays {
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                if let Some((client, data)) = turn.from_peer(relay, from, &buf[..n]) {
                    out.push(Out::ToClient(client, data));
                }
            }
        }
        for o in out {
            match o {
                Out::ToClient(client, data) => {
                    let _ = listen.send_to(&data, client);
                }
                Out::FromRelay { relay, to, bytes } => {
                    let _ = relays[&relay].send_to(&bytes, to);
                }
            }
        }
    };
    let url = format!("turn:{}", listen.local_addr().unwrap());

    let config = RtcConfig::new()
        .bind("127.0.0.1:0".parse().unwrap())
        .turn(&url, "alice", "secret")
        .relay_only();
    let (host_side, client_side) = (Signaling::new(), Signaling::new());
    let mut host = NetMessages::host_rtc(host_side.clone(), config.clone(), 2).unwrap();
    let mut client = NetMessages::join_rtc(client_side.clone(), 1, config).unwrap();
    let mut message = None;
    // Simulated time; the kernel delivers loopback packets at once.
    for _ in 0..5000 {
        poll_turn(&mut turn);
        host.poll_elapsed(STEP).unwrap();
        poll_turn(&mut turn);
        client.poll_elapsed(STEP).unwrap();
        while let Some((_, bytes)) = client_side.next_outgoing() {
            host_side.deliver(2, &bytes);
        }
        while let Some((_, bytes)) = host_side.next_outgoing() {
            client_side.deliver(1, &bytes);
        }
        if client.events().any(|e| e == MessageEvent::Connected(PeerId::HOST)) {
            client.send(PeerId::HOST, Lane::Reliable, b"through the relay").unwrap();
        }
        if let Some(MessageEvent::Message { bytes, .. }) =
            host.events().find(|e| matches!(e, MessageEvent::Message { .. }))
        {
            message = Some(bytes);
            break;
        }
    }
    assert_eq!(message.as_deref(), Some(b"through the relay".as_slice()));
    assert!(turn.relayed_to_peers > 0 && turn.relayed_to_clients > 0);
    assert_eq!(turn.relays().count(), 2, "both ends relayed, as relay_only asks");
}

#[test]
fn a_host_takes_several_clients_and_a_rejoining_one_gets_a_fresh_connection() {
    let addrs = [
        "203.0.113.1:5000",
        "203.0.113.2:5000",
        "203.0.113.3:5000",
        "203.0.113.4:5000",
    ];
    let mut s = Session::new(open_network(), &addrs, false, 4);
    s.connect(500).expect("all three connect");
    for client in 1..=3 {
        let peer = s.host_peer_of(client);
        s.net(0).send(peer, Lane::Reliable, &[client as u8]).unwrap();
    }
    s.step(20);
    for client in 1..=3 {
        assert_eq!(s.messages(client, Lane::Reliable), [vec![client as u8]]);
    }

    // Client 2 leaves (its endpoint is dropped: a Bye goes out over
    // signaling) and joins again with a new endpoint.
    s.nets[2] = None;
    s.step(1);
    let agent = agent(&s.net, "203.0.113.3:5001", false);
    s.nets[2] = Some(NetMessages::rtc_join_with(s.signaling[2].clone(), agent, 1));
    s.logs[2].clear();
    for _ in 0..500 {
        if s.connected(2) == 1 {
            break;
        }
        s.step(1);
    }
    assert_eq!(s.connected(2), 1, "the rejoin connects");
    s.net(2).send(PeerId::HOST, Lane::Reliable, b"back").unwrap();
    s.step(20);
    assert_eq!(s.messages(0, Lane::Reliable), [b"back".to_vec()]);
}

#[test]
fn a_full_host_leaves_extra_offers_unanswered_and_strangers_are_ignored() {
    // max_clients 1: one client plus the spare connections being set up.
    let addrs: Vec<String> = (1..=11).map(|i| format!("203.0.113.{i}:5000")).collect();
    let addrs: Vec<&str> = addrs.iter().map(String::as_str).collect();
    let mut s = Session::new(open_network(), &addrs, false, 1);
    s.step(300);
    let answered = (1..addrs.len()).filter(|&i| s.connected(i) == 1).count();
    assert_eq!(answered, 1, "one client in");
    assert_eq!(s.connected(0), 1);

    // A client of host 1 ignores signals from anyone else, and junk.
    let signaling = Signaling::new();
    let mut client = NetMessages::rtc_join_with(signaling.clone(), agent(&s.net, "203.0.113.50:5000", false), 1);
    let (_, offer) = signaling.next_outgoing().unwrap();
    signaling.deliver(7, &offer);
    signaling.deliver(1, b"PRTC\x01{\"s\":1,\"t\":\"answer\",\"sdp\":\"not sdp\"}");
    for _ in 0..10 {
        client.poll_elapsed(STEP).unwrap();
    }
    assert_eq!(client.connected_peers(), 0);
    assert!(client.events().all(|e| !matches!(e, MessageEvent::Connected(_))));
}

#[test]
fn a_client_joins_over_real_udp_sockets_on_loopback() {
    use super::rtc::RtcConfig;
    let config = RtcConfig::new().bind("127.0.0.1:0".parse().unwrap());
    let (host_side, client_side) = (Signaling::new(), Signaling::new());
    let mut host = NetMessages::host_rtc(host_side.clone(), config.clone(), 2).unwrap();
    let mut client = NetMessages::join_rtc(client_side.clone(), 1, config).unwrap();
    let mut got = None;
    // Simulated time; the kernel delivers loopback packets at once. No sleep:
    // a few thousand polls at most, well under a second of real time.
    for _ in 0..3000 {
        host.poll_elapsed(STEP).unwrap();
        client.poll_elapsed(STEP).unwrap();
        while let Some((_, bytes)) = client_side.next_outgoing() {
            host_side.deliver(2, &bytes);
        }
        while let Some((_, bytes)) = host_side.next_outgoing() {
            client_side.deliver(1, &bytes);
        }
        for event in client.events() {
            if event == MessageEvent::Connected(PeerId::HOST) {
                got = Some(());
            }
        }
        host.events().for_each(drop);
        if got.is_some() {
            break;
        }
    }
    assert!(got.is_some(), "connected over loopback");
    client.send(PeerId::HOST, Lane::Reliable, b"over udp").unwrap();
    let mut message = None;
    for _ in 0..3000 {
        client.poll_elapsed(STEP).unwrap();
        host.poll_elapsed(STEP).unwrap();
        if let Some(MessageEvent::Message { bytes, .. }) =
            host.events().find(|e| matches!(e, MessageEvent::Message { .. }))
        {
            message = Some(bytes);
            break;
        }
    }
    assert_eq!(message.as_deref(), Some(b"over udp".as_slice()));
}

/// A data-channel-only offer in the format Chromium-based browsers send
/// (hand-written to that format, not captured from a browser: no browser
/// runs in these tests).
const BROWSER_OFFER: &str = "v=0\r\n\
o=- 4215775240449105457 2 IN IP4 127.0.0.1\r\n\
s=-\r\n\
t=0 0\r\n\
a=group:BUNDLE 0\r\n\
a=extmap-allow-mixed\r\n\
a=msid-semantic: WMS\r\n\
m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n\
c=IN IP4 0.0.0.0\r\n\
a=ice-ufrag:x9Qd\r\n\
a=ice-pwd:Zk3qkW8bbR0n1vGqC3fYjH7m\r\n\
a=ice-options:trickle\r\n\
a=fingerprint:sha-256 3A:5B:7C:1D:2E:9F:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:10:20:30:40:50:60:70:80:90:A0\r\n\
a=setup:actpass\r\n\
a=mid:0\r\n\
a=sctp-port:5000\r\n\
a=max-message-size:262144\r\n";

#[test]
fn a_browser_style_offer_is_answered_and_its_candidates_are_read_or_skipped() {
    let net = open_network();
    let mut host = agent(&net, "203.0.113.1:5000", false);
    host.signal(
        1,
        super::rtc::Signal::Offer {
            sdp: BROWSER_OFFER.into(),
        },
    );
    let Some((1, super::rtc::Signal::Answer { sdp })) = host.take_signal() else {
        panic!("no answer to a browser's offer");
    };
    assert!(
        sdp.contains("webrtc-datachannel") && sdp.contains("a=sctp-port:5000"),
        "{sdp}"
    );
    assert!(!host.is_closed(1));
    // What browsers trickle: an mDNS host candidate (skipped: it can't be
    // resolved here) and a reflexive one with the browser's extensions.
    for candidate in [
        "candidate:1467250027 1 udp 2122260223 3c3e5f2a-79a1-4f4b-9f0e-6b2d1a7c8e90.local 54321 typ host generation 0 ufrag x9Qd network-id 1",
        "candidate:842163049 1 udp 1677729535 198.51.100.7 61234 typ srflx raddr 0.0.0.0 rport 0 generation 0 ufrag x9Qd network-id 1 network-cost 10",
    ] {
        host.signal(
            1,
            super::rtc::Signal::Candidate {
                candidate: candidate.into(),
            },
        );
        host.advance(STEP);
        assert!(!host.is_closed(1), "{candidate}");
    }
    // The reflexive candidate was read: the host's ICE checks go to it.
    host.advance(STEP * 10);
    let tried = &net.lock().unwrap().tried;
    assert!(
        tried.iter().any(|to| to.to_string() == "198.51.100.7:61234"),
        "checks to the browser's address: {tried:?}"
    );
    assert!(
        tried.iter().all(|to| !to.ip().is_unspecified()),
        "nothing for the .local name"
    );
}
