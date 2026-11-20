//! Steam adapter for the message protocol. Tests replace only the SDK boundary.
use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use steamworks::networking_types::{NetworkingIdentity, SendFlags};
use steamworks::{Client, LobbyId, LobbyType, SteamId};

use super::messages::PacketTransport;
use super::steam_transport::{HOST, addr, steam_id};
use super::{NetMessages, PeerId};

const CHANNEL: u32 = 12701; // independent of lockstep's channel zero
const PACKET_LIMIT: usize = 1500; // renet's MTU plus our session envelope
const PROTOCOL: &str = "ponko-messages-1";
#[derive(Default)]
struct LobbyResult {
    result: Option<Result<LobbyId, String>>,
    cancelled: bool,
}
type Pending = Arc<Mutex<LobbyResult>>;

fn complete(pending: &Pending, value: Result<LobbyId, String>, leave: impl FnOnce(LobbyId)) {
    let mut pending = pending.lock().expect("lobby result");
    if pending.cancelled {
        if let Ok(id) = value {
            leave(id);
        }
    } else {
        pending.result = Some(value);
    }
}

/// Visibility of a Steam lobby (feature `steam`).
/// ```
/// let visibility = ponko::net::SteamLobbyVisibility::FriendsOnly;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SteamLobbyVisibility {
    /// Only friends may discover and join.
    FriendsOnly,
    /// Discoverable by all players.
    Public,
    /// Join through an invitation.
    Private,
}

// No SDK initialization happens here: the application supplies a Client.
// The same boundary covers lobby callbacks and packet I/O in fake tests.
trait SteamApi: Send + Sync {
    fn pump(&self);
    fn create(&self, visibility: SteamLobbyVisibility, max: u32, result: Pending);
    fn join(&self, lobby: LobbyId, result: Pending);
    fn leave(&self, lobby: LobbyId);
    fn owner(&self, lobby: LobbyId) -> SteamId;
    fn members(&self, lobby: LobbyId) -> Vec<SteamId>;
    fn metadata(&self, lobby: LobbyId, key: &str) -> Option<String>;
    fn set_metadata(&self, lobby: LobbyId, key: &str, value: &str) -> bool;
    fn invite(&self, lobby: LobbyId);
    fn send(&self, peer: SteamId, bytes: &[u8]) -> io::Result<()>;
    fn receive(&self) -> Vec<(SteamId, Vec<u8>)>;
}
struct Real(Client);
impl SteamApi for Real {
    fn pump(&self) {
        self.0.run_callbacks();
    }
    fn create(&self, visibility: SteamLobbyVisibility, max: u32, result: Pending) {
        let kind = match visibility {
            SteamLobbyVisibility::FriendsOnly => LobbyType::FriendsOnly,
            SteamLobbyVisibility::Public => LobbyType::Public,
            SteamLobbyVisibility::Private => LobbyType::Private,
        };
        let client = self.0.clone();
        self.0.matchmaking().create_lobby(kind, max, move |r| {
            complete(&result, r.map_err(|e| e.to_string()), |id| {
                client.matchmaking().leave_lobby(id)
            });
        });
    }
    fn join(&self, lobby: LobbyId, result: Pending) {
        let client = self.0.clone();
        self.0.matchmaking().join_lobby(lobby, move |r| {
            complete(&result, r.map_err(|()| "Steam lobby join failed".into()), |id| {
                client.matchmaking().leave_lobby(id)
            });
        });
    }
    fn leave(&self, lobby: LobbyId) {
        self.0.matchmaking().leave_lobby(lobby);
    }
    fn owner(&self, lobby: LobbyId) -> SteamId {
        self.0.matchmaking().lobby_owner(lobby)
    }
    fn members(&self, lobby: LobbyId) -> Vec<SteamId> {
        self.0.matchmaking().lobby_members(lobby)
    }
    fn metadata(&self, lobby: LobbyId, key: &str) -> Option<String> {
        self.0.matchmaking().lobby_data(lobby, key)
    }
    fn set_metadata(&self, lobby: LobbyId, key: &str, value: &str) -> bool {
        self.0.matchmaking().set_lobby_data(lobby, key, value)
    }
    fn invite(&self, lobby: LobbyId) {
        self.0.friends().activate_invite_dialog(lobby);
    }
    fn send(&self, peer: SteamId, bytes: &[u8]) -> io::Result<()> {
        self.0
            .networking_messages()
            .send_message_to_user(
                NetworkingIdentity::new_steam_id(peer),
                SendFlags::UNRELIABLE_NO_NAGLE | SendFlags::AUTO_RESTART_BROKEN_SESSION,
                bytes,
                CHANNEL,
            )
            .map_err(|e| io::Error::other(format!("Steam send: {e}")))
    }
    fn receive(&self) -> Vec<(SteamId, Vec<u8>)> {
        self.0
            .networking_messages()
            .receive_messages_on_channel(CHANNEL, 256)
            .into_iter()
            .filter_map(|m| Some((m.identity_peer().steam_id()?, m.data().to_vec())))
            .collect()
    }
}

/// An asynchronously created/joined Steam lobby. Call `poll` each frame;
/// `id` is `None` until Steam completes the request. Drop or `leave` leaves it.
/// Metadata is cached by Steam; remote changes become visible after callbacks.
/// ```no_run
/// use ponko::prelude::*;
/// let client = steamworks::Client::init_app(480).unwrap();
/// let mut lobby = ponko::net::SteamLobby::create(&client, ponko::net::SteamLobbyVisibility::FriendsOnly, 4).unwrap();
/// lobby.poll().unwrap();
/// if lobby.id().is_some() { lobby.set_metadata("version", "1").unwrap(); }
/// ```
pub struct SteamLobby {
    api: Arc<dyn SteamApi>,
    pending: Pending,
    left: bool,
}
impl SteamLobby {
    fn start(api: Arc<dyn SteamApi>, join: Option<LobbyId>, visibility: SteamLobbyVisibility, max: u32) -> Self {
        let pending = Pending::default();
        if let Some(id) = join {
            api.join(id, pending.clone());
        } else {
            api.create(visibility, max, pending.clone());
        }
        Self {
            api,
            pending,
            left: false,
        }
    }
    /// Create a lobby for 1..=250 members, including its owner.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if `max_members` is outside 1..=250. Steam's own
    /// failures arrive later, through [`poll`](Self::poll).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # let client = steamworks::Client::init_app(480).unwrap();
    /// let lobby = ponko::net::SteamLobby::create(&client, ponko::net::SteamLobbyVisibility::Private, 2)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn create(client: &Client, visibility: SteamLobbyVisibility, max_members: u32) -> io::Result<Self> {
        if !(1..=250).contains(&max_members) {
            return Err(input("max_members must be 1..=250"));
        }
        Ok(Self::start(
            Arc::new(Real(client.clone())),
            None,
            visibility,
            max_members,
        ))
    }
    /// Join by lobby id; completion/errors arrive through `poll`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if the lobby id is zero. A lobby that is full, closed
    /// or gone arrives later, through [`poll`](Self::poll).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # let client = steamworks::Client::init_app(480).unwrap();
    /// let lobby = ponko::net::SteamLobby::join(&client, steamworks::LobbyId::from_raw(1))?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn join(client: &Client, id: LobbyId) -> io::Result<Self> {
        if id.raw() == 0 {
            return Err(input("lobby id is zero"));
        }
        Ok(Self::start(
            Arc::new(Real(client.clone())),
            Some(id),
            SteamLobbyVisibility::Private,
            0,
        ))
    }
    /// Run callbacks and report asynchronous errors. Never blocks.
    ///
    /// # Errors
    ///
    /// The failure of the create or join request, once Steam reports it.
    ///
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.poll(); }
    /// ```
    pub fn poll(&self) -> io::Result<()> {
        self.api.pump();
        match self.pending.lock().expect("lobby result").result.as_ref() {
            Some(Err(e)) => Err(io::Error::other(e.clone())),
            _ => Ok(()),
        }
    }
    /// Current id, or `None` while pending/after leaving.
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.id(); }
    /// ```
    pub fn id(&self) -> Option<LobbyId> {
        if self.left {
            return None;
        }
        self.pending
            .lock()
            .expect("lobby result")
            .result
            .as_ref()?
            .as_ref()
            .ok()
            .copied()
    }
    fn ready(&self) -> io::Result<LobbyId> {
        self.id()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "lobby is not ready"))
    }
    /// Current members, as Steam identities; empty while pending.
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.members(); }
    /// ```
    pub fn members(&self) -> Vec<SteamId> {
        self.id().map_or_else(Vec::new, |id| self.api.members(id))
    }
    /// Read cached lobby metadata; missing keys return `None`.
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.metadata("version"); }
    /// ```
    pub fn metadata(&self, key: &str) -> Option<String> {
        if !valid_key(key) {
            return None;
        }
        self.api.metadata(self.id()?, key)
    }
    /// Set metadata (owner only). Keys are 1..=254 bytes, values at most
    /// 8191 bytes; neither may contain NUL. Check version compatibility before play.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a key or value outside those limits,
    /// `NotConnected` while the lobby is still being created or joined (or
    /// after leaving), and an error if Steam refuses (only the owner may set
    /// metadata).
    ///
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.set_metadata("version", "1"); }
    /// ```
    pub fn set_metadata(&self, key: &str, value: &str) -> io::Result<()> {
        if !valid_key(key) || value.len() > 8191 || value.contains('\0') {
            return Err(input("invalid lobby metadata"));
        }
        if !self.api.set_metadata(self.ready()?, key, value) {
            return Err(io::Error::other("could not set lobby metadata (owner only)"));
        }
        Ok(())
    }
    /// Open the overlay invite dialog. Existing invite argument/wait helpers
    /// also work for these lobbies.
    ///
    /// # Errors
    ///
    /// `NotConnected` while the lobby is still being created or joined (or
    /// after leaving).
    ///
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.invite(); }
    /// ```
    pub fn invite(&self) -> io::Result<()> {
        self.api.invite(self.ready()?);
        Ok(())
    }
    /// Leave once. A pending request must finish first.
    ///
    /// # Errors
    ///
    /// `WouldBlock` while the lobby is still being created or joined, or the
    /// lobby's failure if it failed.
    ///
    /// ```no_run
    /// # fn f(lobby: &mut ponko::net::SteamLobby) { let _ = lobby.leave(); }
    /// ```
    pub fn leave(&mut self) -> io::Result<()> {
        self.poll()?;
        if self.left {
            return Ok(());
        }
        let id = self
            .id()
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "lobby request pending"))?;
        self.api.leave(id);
        self.left = true;
        Ok(())
    }
}
impl Drop for SteamLobby {
    fn drop(&mut self) {
        let mut pending = self.pending.lock().expect("lobby result");
        pending.cancelled = true;
        if !self.left
            && let Some(Ok(id)) = pending.result
        {
            self.api.leave(id);
        }
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() < 255 && !key.contains('\0')
}
fn input(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}

struct Relay {
    lobby: SteamLobby,
    hosting: bool,
    owner: Option<SteamId>,
    inbox: VecDeque<(SteamId, Vec<u8>)>,
    published: bool,
}
impl Relay {
    fn new(lobby: SteamLobby, hosting: bool) -> Self {
        Self {
            lobby,
            hosting,
            owner: None,
            inbox: VecDeque::new(),
            published: false,
        }
    }
    fn prepare(&mut self) -> io::Result<()> {
        self.lobby.poll()?;
        if let Some(id) = self.lobby.id() {
            if self.hosting && !self.published {
                self.lobby.set_metadata("ponko_protocol", PROTOCOL)?;
                self.published = true;
            }
            if !self.hosting {
                match self.lobby.metadata("ponko_protocol").as_deref() {
                    Some(PROTOCOL) => self.owner = Some(self.lobby.api.owner(id)),
                    Some(_) => return Err(io::Error::other("incompatible message protocol")),
                    None => return Ok(()), // owner may still be publishing
                }
            }
        }
        Ok(())
    }
}
impl PacketTransport for Relay {
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
        if bytes.len() > PACKET_LIMIT {
            return Err(input("Steam packet exceeds message MTU"));
        }
        self.prepare()?;
        let peer = if to == HOST { self.owner } else { steam_id(to) };
        let Some(peer) = peer else { return Ok(false) };
        if !self.lobby.members().contains(&peer) {
            return Ok(false);
        }
        self.lobby.api.send(peer, bytes)?;
        Ok(true)
    }
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        self.prepare()?;
        if self.inbox.is_empty() {
            self.inbox.extend(self.lobby.api.receive());
        }
        while let Some((peer, bytes)) = self.inbox.pop_front() {
            if bytes.len() > buf.len()
                || bytes.len() > PACKET_LIMIT
                || !self.lobby.members().contains(&peer)
                || (!self.hosting && Some(peer) != self.owner)
            {
                continue;
            }
            buf[..bytes.len()].copy_from_slice(&bytes);
            return Ok(Some((
                if Some(peer) == self.owner { HOST } else { addr(peer) },
                bytes.len(),
            )));
        }
        Ok(None)
    }
    fn lobby(&self) -> Option<&SteamLobby> {
        Some(&self.lobby)
    }
}
impl NetMessages {
    /// Host message traffic through Steam relays, creating a friends-only
    /// lobby. `max_clients` is 1..=249 (Steam permits 250 lobby members).
    /// Use one message endpoint per Steam client; lockstep uses a separate channel.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if `max_clients` is outside 1..=249.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// let client = steamworks::Client::init_app(480).unwrap();
    /// let mut host = ponko::net::NetMessages::steam_host(&client, 3)?;
    /// host.poll()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn steam_host(client: &Client, max_clients: usize) -> io::Result<Self> {
        if !(1..=249).contains(&max_clients) {
            return Err(input("max_clients must be 1..=249"));
        }
        let lobby = SteamLobby::create(client, SteamLobbyVisibility::FriendsOnly, max_clients as u32 + 1)?;
        Ok(Self::steam_endpoint(client, lobby, true, max_clients))
    }
    /// Join a message host by lobby id. Poll until `Connected`; lobby failures
    /// are returned by `poll`. Host migration requires a new endpoint.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if the lobby id is zero.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # let client = steamworks::Client::init_app(480).unwrap();
    /// let net = ponko::net::NetMessages::steam_join(&client, steamworks::LobbyId::from_raw(1))?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn steam_join(client: &Client, lobby: LobbyId) -> io::Result<Self> {
        Ok(Self::steam_endpoint(client, SteamLobby::join(client, lobby)?, false, 1))
    }
    fn steam_endpoint(client: &Client, lobby: SteamLobby, hosting: bool, max: usize) -> Self {
        // Steam authenticates identities. Session membership and the normal
        // message handshake govern delivery; receiving also filters members.
        client.networking_messages().session_request_callback(|r| {
            r.accept();
        });
        Self::transported(
            Box::new(Relay::new(lobby, hosting)),
            if hosting { None } else { Some(HOST) },
            max,
        )
    }
    /// This endpoint's lobby, for metadata, membership and overlay invites.
    /// Returns `None` for UDP and loopback endpoints.
    /// ```
    /// assert!(ponko::net::NetMessages::loopback(2)[0].steam_lobby().is_none());
    /// ```
    pub fn steam_lobby(&self) -> Option<&SteamLobby> {
        self.transport_lobby()
    }
    /// Steam identity corresponding to a connected session-local peer.
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(ponko::net::NetMessages::loopback(2)[0].steam_peer(ponko::net::PeerId::HOST).is_none());
    /// ```
    pub fn steam_peer(&self, peer: PeerId) -> Option<SteamId> {
        self.transport_peer_addr(peer).and_then(|a| {
            if a == HOST {
                let lobby = self.steam_lobby()?;
                Some(lobby.api.owner(lobby.id()?))
            } else {
                steam_id(a)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Lane;
    use crate::net::MAX_MESSAGE_BYTES;
    use crate::net::MessageEvent;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[derive(Default)]
    struct Room {
        owner: u64,
        max: u32,
        visibility: Option<SteamLobbyVisibility>,
        members: Vec<u64>,
        metadata: BTreeMap<String, String>,
    }
    #[derive(Default)]
    struct State {
        rooms: BTreeMap<u64, Room>,
        packets: BTreeMap<u64, VecDeque<(SteamId, Vec<u8>)>>,
        invites: usize,
        largest: usize,
    }
    struct Fake {
        me: u64,
        state: Arc<Mutex<State>>,
    }
    impl SteamApi for Fake {
        fn pump(&self) {}
        fn create(&self, visibility: SteamLobbyVisibility, max: u32, result: Pending) {
            self.state.lock().unwrap().rooms.insert(
                self.me,
                Room {
                    owner: self.me,
                    max,
                    visibility: Some(visibility),
                    members: vec![self.me],
                    ..Room::default()
                },
            );
            complete(&result, Ok(LobbyId::from_raw(self.me)), |id| self.leave(id));
        }
        fn join(&self, lobby: LobbyId, result: Pending) {
            let mut s = self.state.lock().unwrap();
            let r = match s.rooms.get_mut(&lobby.raw()) {
                Some(r) if r.members.len() < r.max as usize => {
                    r.members.push(self.me);
                    Ok(lobby)
                }
                _ => Err("lobby unavailable".into()),
            };
            drop(s);
            complete(&result, r, |id| self.leave(id));
        }
        fn leave(&self, lobby: LobbyId) {
            if let Some(r) = self.state.lock().unwrap().rooms.get_mut(&lobby.raw()) {
                r.members.retain(|&p| p != self.me);
            }
        }
        fn owner(&self, lobby: LobbyId) -> SteamId {
            SteamId::from_raw(self.state.lock().unwrap().rooms[&lobby.raw()].owner)
        }
        fn members(&self, lobby: LobbyId) -> Vec<SteamId> {
            self.state.lock().unwrap().rooms[&lobby.raw()]
                .members
                .iter()
                .map(|&p| SteamId::from_raw(p))
                .collect()
        }
        fn metadata(&self, lobby: LobbyId, key: &str) -> Option<String> {
            self.state.lock().unwrap().rooms[&lobby.raw()]
                .metadata
                .get(key)
                .cloned()
        }
        fn set_metadata(&self, lobby: LobbyId, key: &str, value: &str) -> bool {
            let mut s = self.state.lock().unwrap();
            let r = s.rooms.get_mut(&lobby.raw()).unwrap();
            if r.owner != self.me {
                return false;
            }
            r.metadata.insert(key.into(), value.into());
            true
        }
        fn invite(&self, _: LobbyId) {
            self.state.lock().unwrap().invites += 1;
        }
        fn send(&self, peer: SteamId, bytes: &[u8]) -> io::Result<()> {
            let mut s = self.state.lock().unwrap();
            s.largest = s.largest.max(bytes.len());
            s.packets
                .entry(peer.raw())
                .or_default()
                .push_back((SteamId::from_raw(self.me), bytes.to_vec()));
            Ok(())
        }
        fn receive(&self) -> Vec<(SteamId, Vec<u8>)> {
            let mut s = self.state.lock().unwrap();
            let q = s.packets.entry(self.me).or_default();
            // Deliberate reordering, still bounded like the SDK batch.
            (0..256).filter_map(|_| q.pop_back()).collect()
        }
    }
    fn api(state: &Arc<Mutex<State>>, me: u64) -> Arc<dyn SteamApi> {
        Arc::new(Fake {
            me,
            state: state.clone(),
        })
    }
    fn endpoints(players: usize) -> (Vec<NetMessages>, Arc<Mutex<State>>) {
        let s = Arc::new(Mutex::new(State::default()));
        let nets = (1..=players)
            .map(|me| {
                let lobby = SteamLobby::start(
                    api(&s, me as u64),
                    if me == 1 { None } else { Some(LobbyId::from_raw(1)) },
                    SteamLobbyVisibility::FriendsOnly,
                    players as u32,
                );
                NetMessages::transported(
                    Box::new(Relay::new(lobby, me == 1)),
                    if me == 1 { None } else { Some(HOST) },
                    players - 1,
                )
            })
            .collect();
        (nets, s)
    }
    fn poll(nets: &mut [NetMessages], steps: usize) {
        for _ in 0..steps {
            for n in &mut *nets {
                n.poll_elapsed(Duration::from_millis(10)).unwrap();
            }
        }
    }
    #[test]
    fn dropping_a_pending_lobby_leaves_when_callback_completes() {
        let s = Arc::new(Mutex::new(State::default()));
        let api = api(&s, 1);
        let pending = Pending::default();
        let lobby = SteamLobby {
            api: api.clone(),
            pending: pending.clone(),
            left: false,
        };
        drop(lobby);
        api.create(SteamLobbyVisibility::Private, 2, pending);
        assert!(s.lock().unwrap().rooms[&1].members.is_empty());
    }
    #[test]
    fn replicates_over_the_steam_relay() {
        use crate::net::replication_conformance::{Session, declarations, exercise};
        let (nets, _state) = endpoints(3);
        exercise(Session::over(nets, declarations, false), 400, 200);
    }
    #[test]
    fn two_and_three_peers_exchange_all_lanes_and_disconnect() {
        for players in [2, 3] {
            let (mut nets, state) = endpoints(players);
            poll(&mut nets, 30);
            let peers: Vec<_> = nets[0]
                .events()
                .filter_map(|e| {
                    if let MessageEvent::Connected(p) = e {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(peers.len(), players - 1);
            for n in &mut nets[1..] {
                assert_eq!(
                    n.events().collect::<Vec<_>>(),
                    vec![MessageEvent::Connected(PeerId::HOST)]
                );
            }
            for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
                for peer in &peers {
                    let id = nets[0].steam_peer(*peer).unwrap().raw();
                    nets[0].send(*peer, lane, &[id as u8, 42]).unwrap();
                }
                for (i, n) in nets[1..].iter_mut().enumerate() {
                    n.send(PeerId::HOST, lane, &[(i + 2) as u8, 24]).unwrap();
                }
                poll(&mut nets, 30);
                let got: Vec<_> = nets[0].events().collect();
                assert_eq!(got.len(), players - 1);
                for e in got {
                    let MessageEvent::Message { peer, lane: l, bytes } = e else {
                        panic!("expected message");
                    };
                    assert_eq!(l, lane);
                    assert_eq!(nets[0].steam_peer(peer).unwrap().raw(), u64::from(bytes[0]));
                    assert_eq!(bytes[1], 24);
                }
                for (i, n) in nets[1..].iter_mut().enumerate() {
                    assert!(
                        matches!(n.next_event(), Some(MessageEvent::Message { lane: l, bytes, .. }) if l == lane && bytes == vec![(i + 2) as u8, 42])
                    );
                }
            }
            nets[1].disconnect(PeerId::HOST).unwrap();
            poll(&mut nets, 3);
            assert!(nets[0].events().any(|e| matches!(e, MessageEvent::Disconnected(_, _))));
            assert!(state.lock().unwrap().largest <= PACKET_LIMIT);
        }
    }
    #[test]
    fn fragmented_payload_crosses_fake_steam_relay() {
        let (mut nets, s) = endpoints(2);
        poll(&mut nets, 30);
        for n in &mut nets {
            n.events().for_each(drop);
        }
        for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
            let bytes = vec![73; MAX_MESSAGE_BYTES];
            nets[1].send(PeerId::HOST, lane, &bytes).unwrap();
            poll(&mut nets, 150);
            assert!(
                nets[0]
                    .events()
                    .any(|e| matches!(e, MessageEvent::Message { lane: l, bytes: b, .. } if l == lane && b == bytes))
            );
        }
        assert!(s.lock().unwrap().largest <= PACKET_LIMIT);
    }
    #[test]
    fn lobby_members_metadata_invites_and_leave() {
        for visibility in [
            SteamLobbyVisibility::FriendsOnly,
            SteamLobbyVisibility::Public,
            SteamLobbyVisibility::Private,
        ] {
            let s = Arc::new(Mutex::new(State::default()));
            let mut host = SteamLobby::start(api(&s, 1), None, visibility, 2);
            let mut join = SteamLobby::start(api(&s, 2), host.id(), visibility, 0);
            assert_eq!(s.lock().unwrap().rooms[&1].visibility, Some(visibility));
            host.poll().unwrap();
            join.poll().unwrap();
            assert_eq!(host.members().len(), 2);
            host.set_metadata("version", "1.2").unwrap();
            host.set_metadata("protocol", "7").unwrap();
            assert_eq!(join.metadata("version").as_deref(), Some("1.2"));
            assert!(join.set_metadata("version", "bad").is_err());
            assert!(host.set_metadata("a\0b", "x").is_err());
            let full = SteamLobby::start(api(&s, 3), host.id(), visibility, 0);
            assert!(full.poll().is_err());
            host.invite().unwrap();
            assert_eq!(s.lock().unwrap().invites, 1);
            join.leave().unwrap();
            assert_eq!(host.members().len(), 1);
            assert!(join.id().is_none());
            host.leave().unwrap();
            assert!(s.lock().unwrap().rooms[&1].members.is_empty());
        }
    }
    #[test]
    fn incompatible_lobby_and_nonmembers_cannot_route() {
        let (mut nets, s) = endpoints(3);
        nets.pop(); // leave one host slot free: an outsider HELLO must still be rejected
        poll(&mut nets, 30);
        for n in &mut nets {
            n.events().for_each(drop);
        }
        let mut hello = b"CRM1".to_vec();
        hello.push(1); // valid HELLO: would connect without membership filtering
        hello.extend_from_slice(&1u64.to_le_bytes());
        hello.extend_from_slice(&0u64.to_le_bytes());
        s.lock()
            .unwrap()
            .packets
            .entry(1)
            .or_default()
            .push_back((SteamId::from_raw(99), hello));
        poll(&mut nets, 1);
        assert!(nets[0].events().next().is_none());
        s.lock()
            .unwrap()
            .rooms
            .get_mut(&1)
            .unwrap()
            .metadata
            .insert("ponko_protocol".into(), "other".into());
        assert!(nets[1].poll_elapsed(Duration::ZERO).is_err());
    }
    #[test]
    fn protocol_metadata_is_published_and_unknown_versions_are_rejected() {
        let (mut nets, state) = endpoints(2);
        poll(&mut nets, 30);
        let metadata = state.lock().unwrap().rooms[&1].metadata.clone();
        assert_eq!(
            metadata.get("ponko_protocol").map(String::as_str),
            Some("ponko-messages-1")
        );
        state
            .lock()
            .unwrap()
            .rooms
            .get_mut(&1)
            .unwrap()
            .metadata
            .remove("ponko_protocol");
        assert!(
            nets[1].poll_elapsed(Duration::ZERO).is_ok(),
            "a joiner waits while the host has not published yet"
        );
        {
            let mut state = state.lock().unwrap();
            let metadata = &mut state.rooms.get_mut(&1).unwrap().metadata;
            metadata.insert("ponko_protocol".into(), "ponko-messages-2".into());
        }
        assert!(
            nets[1].poll_elapsed(Duration::ZERO).is_err(),
            "unknown protocol versions are rejected"
        );
    }
}
