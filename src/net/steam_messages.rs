//! Steam adapter for the message protocol. Tests replace only the SDK boundary.
//!
//! Two carriers move the packets between lobby members: Steam Networking
//! Messages (connectionless, the default) and Steam Networking Sockets
//! (connections the host accepts, relayed through Valve's network). The host
//! picks one and names it in the lobby's metadata; a joiner follows it.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use steamworks::LobbyType;
use steamworks::networking_types::{NetworkingIdentity, SendFlags};

use super::messages::PacketTransport;
use super::steam_transport::{HOST, addr, steam_id};
use super::{NetMessages, PeerId, SteamClient, SteamLobbyId, SteamUserId};

const CHANNEL: u32 = 12701; // independent of lockstep's channel zero
const PACKET_LIMIT: usize = 1500; // renet's MTU plus our session envelope
const PROTOCOL: &str = "ponko-messages-2";
/// The lobby metadata key naming the host's carrier: [`MESSAGES`] or
/// [`SOCKETS`]. A host from before the key existed uses Messages.
const CARRIER_KEY: &str = "ponko_carrier";
const MESSAGES: &str = "messages";
const SOCKETS: &str = "sockets";
/// The Sockets carrier's virtual port, beside lockstep's channel zero and
/// the Messages carrier's channel.
const SOCKETS_PORT: i32 = 12702;
#[derive(Default)]
struct LobbyResult {
    result: Option<Result<SteamLobbyId, String>>,
    cancelled: bool,
}
type Pending = Arc<Mutex<LobbyResult>>;

fn complete(pending: &Pending, value: Result<SteamLobbyId, String>, leave: impl FnOnce(SteamLobbyId)) {
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

// No SDK initialization happens here: the application supplies a SteamClient.
// The same boundary covers lobby callbacks and packet I/O in fake tests.
trait SteamApi: Send + Sync {
    fn pump(&self);
    fn create(&self, visibility: SteamLobbyVisibility, max: u32, result: Pending);
    fn join(&self, lobby: SteamLobbyId, result: Pending);
    fn leave(&self, lobby: SteamLobbyId);
    fn owner(&self, lobby: SteamLobbyId) -> SteamUserId;
    fn members(&self, lobby: SteamLobbyId) -> Vec<SteamUserId>;
    fn metadata(&self, lobby: SteamLobbyId, key: &str) -> Option<String>;
    fn set_metadata(&self, lobby: SteamLobbyId, key: &str, value: &str) -> bool;
    fn invite(&self, lobby: SteamLobbyId);
    fn send(&self, peer: SteamUserId, bytes: &[u8]) -> io::Result<()>;
    fn receive(&self) -> Vec<(SteamUserId, Vec<u8>)>;
}
struct Real(SteamClient);
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
        let client = self.0.inner.clone();
        self.0.inner.matchmaking().create_lobby(kind, max, move |r| {
            complete(
                &result,
                r.map(SteamLobbyId::from_unstable).map_err(|e| e.to_string()),
                |id| client.matchmaking().leave_lobby(id.to_unstable()),
            );
        });
    }
    fn join(&self, lobby: SteamLobbyId, result: Pending) {
        let client = self.0.inner.clone();
        self.0.inner.matchmaking().join_lobby(lobby.to_unstable(), move |r| {
            complete(
                &result,
                r.map(SteamLobbyId::from_unstable)
                    .map_err(|()| "Steam lobby join failed".into()),
                |id| client.matchmaking().leave_lobby(id.to_unstable()),
            );
        });
    }
    fn leave(&self, lobby: SteamLobbyId) {
        self.0.inner.matchmaking().leave_lobby(lobby.to_unstable());
    }
    fn owner(&self, lobby: SteamLobbyId) -> SteamUserId {
        SteamUserId::from_unstable(self.0.inner.matchmaking().lobby_owner(lobby.to_unstable()))
    }
    fn members(&self, lobby: SteamLobbyId) -> Vec<SteamUserId> {
        self.0
            .inner
            .matchmaking()
            .lobby_members(lobby.to_unstable())
            .into_iter()
            .map(SteamUserId::from_unstable)
            .collect()
    }
    fn metadata(&self, lobby: SteamLobbyId, key: &str) -> Option<String> {
        self.0.inner.matchmaking().lobby_data(lobby.to_unstable(), key)
    }
    fn set_metadata(&self, lobby: SteamLobbyId, key: &str, value: &str) -> bool {
        self.0
            .inner
            .matchmaking()
            .set_lobby_data(lobby.to_unstable(), key, value)
    }
    fn invite(&self, lobby: SteamLobbyId) {
        self.0.inner.friends().activate_invite_dialog(lobby.to_unstable());
    }
    fn send(&self, peer: SteamUserId, bytes: &[u8]) -> io::Result<()> {
        self.0
            .inner
            .networking_messages()
            .send_message_to_user(
                NetworkingIdentity::new_steam_id(peer.to_unstable()),
                SendFlags::UNRELIABLE_NO_NAGLE | SendFlags::AUTO_RESTART_BROKEN_SESSION,
                bytes,
                CHANNEL,
            )
            .map_err(|e| io::Error::other(format!("Steam send: {e}")))
    }
    fn receive(&self) -> Vec<(SteamUserId, Vec<u8>)> {
        self.0
            .inner
            .networking_messages()
            .receive_messages_on_channel(CHANNEL, 256)
            .into_iter()
            .filter_map(|m| {
                Some((
                    SteamUserId::from_unstable(m.identity_peer().steam_id()?),
                    m.data().to_vec(),
                ))
            })
            .collect()
    }
}

/// How packets travel between lobby members: Messages or Sockets. Peers are
/// Steam identities; the relay has already filtered them by lobby membership.
trait Carrier: Send {
    /// Before each send and receive: a Sockets host accepts members' and
    /// refuses others' connections and closes those of members who left; a
    /// Sockets client connects to `host`. Messages needs nothing.
    fn maintain(&mut self, _members: &[SteamUserId], _host: Option<SteamUserId>) -> io::Result<()> {
        Ok(())
    }
    /// `Ok(false)`: not submitted (no open connection to `peer` yet), loss.
    fn send(&mut self, peer: SteamUserId, bytes: &[u8]) -> io::Result<bool>;
    fn receive(&mut self) -> Vec<(SteamUserId, Vec<u8>)>;
}

/// Steam Networking Messages: no connections, sessions Steam opens itself.
struct MessagesCarrier(Arc<dyn SteamApi>);
impl Carrier for MessagesCarrier {
    fn send(&mut self, peer: SteamUserId, bytes: &[u8]) -> io::Result<bool> {
        self.0.send(peer, bytes)?;
        Ok(true)
    }
    fn receive(&mut self) -> Vec<(SteamUserId, Vec<u8>)> {
        self.0.receive()
    }
}

/// What a Sockets connection reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocketEvent {
    /// `peer` asks the listening host to connect; answer with `accept` or `close`.
    Request(SteamUserId),
    /// The connection with `peer` is open both ways.
    Connected(SteamUserId),
    /// The connection with `peer` closed (the other side, a timeout, a refusal).
    Closed(SteamUserId),
}

/// The Steam Networking Sockets calls, which tests replace.
trait SocketsApi: Send {
    fn listen(&mut self) -> io::Result<()>;
    fn connect(&mut self, host: SteamUserId) -> io::Result<()>;
    fn events(&mut self) -> Vec<SocketEvent>;
    /// Accepts the pending request of `peer`; `false` if there is none.
    fn accept(&mut self, peer: SteamUserId) -> bool;
    /// Refuses `peer`'s request or closes its connection.
    fn close(&mut self, peer: SteamUserId);
    fn send(&mut self, peer: SteamUserId, bytes: &[u8]) -> bool;
    fn receive(&mut self) -> Vec<(SteamUserId, Vec<u8>)>;
}

/// Steam Networking Sockets: the host listens and accepts lobby members'
/// connections; a client connects to the host. Packets go only over open
/// connections; until one is open, sends are loss and the protocol resends.
struct SocketsCarrier {
    api: Box<dyn SocketsApi>,
    hosting: bool,
    listening: bool,
    /// The host a client connected (or is connecting) to.
    dialed: Option<SteamUserId>,
    open: BTreeSet<SteamUserId>,
}
impl SocketsCarrier {
    fn new(api: Box<dyn SocketsApi>, hosting: bool) -> Self {
        Self {
            api,
            hosting,
            listening: false,
            dialed: None,
            open: BTreeSet::new(),
        }
    }
}
impl Carrier for SocketsCarrier {
    fn maintain(&mut self, members: &[SteamUserId], host: Option<SteamUserId>) -> io::Result<()> {
        if self.hosting && !self.listening {
            self.api.listen()?;
            self.listening = true;
        }
        if !self.hosting
            && let Some(host) = host
            && self.dialed != Some(host)
        {
            self.api.connect(host)?;
            self.dialed = Some(host);
        }
        for event in self.api.events() {
            match event {
                SocketEvent::Request(peer) if self.hosting && members.contains(&peer) => {
                    self.api.accept(peer);
                }
                SocketEvent::Request(peer) => self.api.close(peer),
                SocketEvent::Connected(peer) if self.hosting && !members.contains(&peer) => self.api.close(peer),
                SocketEvent::Connected(peer) if self.hosting || Some(peer) == self.dialed => {
                    self.open.insert(peer);
                }
                SocketEvent::Connected(peer) => self.api.close(peer),
                SocketEvent::Closed(peer) => {
                    self.open.remove(&peer);
                    if Some(peer) == self.dialed {
                        self.dialed = None; // dial again at the next maintain
                    }
                }
            }
        }
        // Members who left the lobby lose their connection.
        let gone: Vec<SteamUserId> = self.open.iter().filter(|p| !members.contains(p)).copied().collect();
        for peer in gone {
            self.api.close(peer);
            self.open.remove(&peer);
            if Some(peer) == self.dialed {
                self.dialed = None;
            }
        }
        Ok(())
    }
    fn send(&mut self, peer: SteamUserId, bytes: &[u8]) -> io::Result<bool> {
        Ok(self.open.contains(&peer) && self.api.send(peer, bytes))
    }
    fn receive(&mut self) -> Vec<(SteamUserId, Vec<u8>)> {
        let mut got = self.api.receive();
        got.retain(|(peer, _)| self.open.contains(peer));
        got
    }
}

/// The real Steam Networking Sockets.
struct RealSockets {
    client: SteamClient,
    listen: Option<steamworks::networking_sockets::ListenSocket>,
    requests: BTreeMap<SteamUserId, steamworks::networking_types::ConnectionRequest>,
    /// Open connections (the host's), or the one being opened (a client's).
    connections: BTreeMap<SteamUserId, steamworks::networking_sockets::NetConnection>,
}
impl RealSockets {
    fn new(client: &SteamClient) -> Self {
        Self {
            client: client.clone(),
            listen: None,
            requests: BTreeMap::new(),
            connections: BTreeMap::new(),
        }
    }
}
fn peer_of(identity: &NetworkingIdentity) -> Option<SteamUserId> {
    identity.steam_id().map(SteamUserId::from_unstable)
}
impl SocketsApi for RealSockets {
    fn listen(&mut self) -> io::Result<()> {
        let socket = self
            .client
            .inner
            .networking_sockets()
            .create_listen_socket_p2p(SOCKETS_PORT, Vec::new())
            .map_err(|_| io::Error::other("Steam refused the listen socket"))?;
        self.listen = Some(socket);
        Ok(())
    }
    fn connect(&mut self, host: SteamUserId) -> io::Result<()> {
        let connection = self
            .client
            .inner
            .networking_sockets()
            .connect_p2p(
                NetworkingIdentity::new_steam_id(host.to_unstable()),
                SOCKETS_PORT,
                Vec::new(),
            )
            .map_err(|_| io::Error::other("Steam refused to connect to the host"))?;
        self.connections.insert(host, connection);
        Ok(())
    }
    fn events(&mut self) -> Vec<SocketEvent> {
        use steamworks::networking_types::{ListenSocketEvent, NetworkingConnectionState as State};
        let mut events = Vec::new();
        if let Some(listen) = &self.listen {
            while let Some(event) = listen.try_receive_event() {
                match event {
                    ListenSocketEvent::Connecting(request) => {
                        if let Some(peer) = peer_of(&request.remote()) {
                            self.requests.insert(peer, request);
                            events.push(SocketEvent::Request(peer));
                        }
                    }
                    ListenSocketEvent::Connected(connected) => {
                        if let Some(peer) = peer_of(&connected.remote()) {
                            self.connections.insert(peer, connected.take_connection());
                            events.push(SocketEvent::Connected(peer));
                        }
                    }
                    ListenSocketEvent::Disconnected(gone) => {
                        if let Some(peer) = peer_of(&gone.remote()) {
                            self.connections.remove(&peer);
                            events.push(SocketEvent::Closed(peer));
                        }
                    }
                }
            }
        } else {
            let mut closed = Vec::new();
            for (peer, connection) in &self.connections {
                while let Some(event) = connection.try_receive_event() {
                    match event.new_state {
                        State::Connected => events.push(SocketEvent::Connected(*peer)),
                        State::ClosedByPeer | State::ProblemDetectedLocally | State::None => closed.push(*peer),
                        _ => {}
                    }
                }
            }
            for peer in closed {
                self.connections.remove(&peer);
                events.push(SocketEvent::Closed(peer));
            }
        }
        events
    }
    fn accept(&mut self, peer: SteamUserId) -> bool {
        self.requests.remove(&peer).is_some_and(|r| r.accept().is_ok())
    }
    fn close(&mut self, peer: SteamUserId) {
        use steamworks::networking_types::{AppNetConnectionEnd, NetConnectionEnd};
        if let Some(request) = self.requests.remove(&peer) {
            request.reject(
                NetConnectionEnd::App(AppNetConnectionEnd::generic_normal()),
                Some("not in the lobby"),
            );
        }
        self.connections.remove(&peer); // dropping closes it
    }
    fn send(&mut self, peer: SteamUserId, bytes: &[u8]) -> bool {
        self.connections
            .get(&peer)
            .is_some_and(|c| c.send_message(bytes, SendFlags::UNRELIABLE_NO_NAGLE).is_ok())
    }
    fn receive(&mut self) -> Vec<(SteamUserId, Vec<u8>)> {
        let mut got = Vec::new();
        for (peer, connection) in &mut self.connections {
            if let Ok(messages) = connection.receive_messages(256) {
                got.extend(messages.into_iter().map(|m| (*peer, m.data().to_vec())));
            }
        }
        got
    }
}

/// Which carrier an endpoint uses: the host's choice, or a joiner's
/// factory that follows the host once its lobby metadata says.
enum Pick {
    Chosen(Box<dyn Carrier>),
    Follow(Box<dyn FnMut(&str) -> Box<dyn Carrier> + Send>),
}

/// An asynchronously created/joined Steam lobby. Call `poll` each frame;
/// `id` is `None` until Steam completes the request. Drop or `leave` leaves it.
/// Metadata is cached by Steam; remote changes become visible after callbacks.
/// ```no_run
/// use ponko::prelude::*;
/// let client = ponko::net::SteamClient::init_app(480).unwrap();
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
    fn start(api: Arc<dyn SteamApi>, join: Option<SteamLobbyId>, visibility: SteamLobbyVisibility, max: u32) -> Self {
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
    /// # let client = ponko::net::SteamClient::init_app(480).unwrap();
    /// let lobby = ponko::net::SteamLobby::create(&client, ponko::net::SteamLobbyVisibility::Private, 2)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn create(client: &SteamClient, visibility: SteamLobbyVisibility, max_members: u32) -> io::Result<Self> {
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
    /// # let client = ponko::net::SteamClient::init_app(480).unwrap();
    /// let lobby = ponko::net::SteamLobby::join(&client, ponko::net::SteamLobbyId::from_raw(1))?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn join(client: &SteamClient, id: SteamLobbyId) -> io::Result<Self> {
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
    pub fn id(&self) -> Option<SteamLobbyId> {
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
    fn ready(&self) -> io::Result<SteamLobbyId> {
        self.id()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "lobby is not ready"))
    }
    /// Current members, as Steam identities; empty while pending.
    /// ```no_run
    /// # fn f(lobby: &ponko::net::SteamLobby) { let _ = lobby.members(); }
    /// ```
    pub fn members(&self) -> Vec<SteamUserId> {
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
    owner: Option<SteamUserId>,
    inbox: VecDeque<(SteamUserId, Vec<u8>)>,
    published: bool,
    carrier: Pick,
    /// The carrier's name in the lobby's metadata (hosts publish it).
    carrier_name: &'static str,
}
impl Relay {
    /// A host with its carrier, named `name` in the lobby's metadata.
    fn host(lobby: SteamLobby, name: &'static str, carrier: Box<dyn Carrier>) -> Self {
        let mut relay = Self::new(lobby, true, Pick::Chosen(carrier));
        relay.carrier_name = name;
        relay
    }
    /// A joiner whose carrier follows the host's (`make` gets its name).
    fn join(lobby: SteamLobby, make: impl FnMut(&str) -> Box<dyn Carrier> + Send + 'static) -> Self {
        Self::new(lobby, false, Pick::Follow(Box::new(make)))
    }
    fn new(lobby: SteamLobby, hosting: bool, carrier: Pick) -> Self {
        Self {
            lobby,
            hosting,
            owner: None,
            inbox: VecDeque::new(),
            published: false,
            carrier,
            carrier_name: MESSAGES,
        }
    }
    /// The open carrier, once the joiner knows which one the host uses.
    fn carrier(&mut self) -> Option<&mut dyn Carrier> {
        match &mut self.carrier {
            Pick::Chosen(c) => Some(c.as_mut()),
            Pick::Follow(_) => None,
        }
    }
    fn prepare(&mut self) -> io::Result<()> {
        self.lobby.poll()?;
        if let Some(id) = self.lobby.id() {
            if self.hosting && !self.published {
                // The carrier first: a joiner waits for the protocol key.
                self.lobby.set_metadata(CARRIER_KEY, self.carrier_name)?;
                self.lobby.set_metadata("ponko_protocol", PROTOCOL)?;
                self.published = true;
            }
            if !self.hosting {
                match self.lobby.metadata("ponko_protocol").as_deref() {
                    Some(PROTOCOL) => self.owner = Some(self.lobby.api.owner(id)),
                    Some(_) => return Err(io::Error::other("incompatible message protocol")),
                    None => return Ok(()), // owner may still be publishing
                }
                if let Pick::Follow(make) = &mut self.carrier {
                    let name = self.lobby.metadata(CARRIER_KEY).unwrap_or_else(|| MESSAGES.into());
                    if name != MESSAGES && name != SOCKETS {
                        return Err(io::Error::other(format!(
                            "the host uses the Steam carrier {name:?}, which this version doesn't know"
                        )));
                    }
                    self.carrier = Pick::Chosen(make(&name));
                }
            }
            let members = self.lobby.members();
            let owner = self.owner;
            if let Some(carrier) = self.carrier() {
                carrier.maintain(&members, owner)?;
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
        match self.carrier() {
            Some(carrier) => carrier.send(peer, bytes),
            None => Ok(false),
        }
    }
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        self.prepare()?;
        if self.inbox.is_empty()
            && let Some(carrier) = self.carrier()
        {
            let got = carrier.receive();
            self.inbox.extend(got);
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
    /// let client = ponko::net::SteamClient::init_app(480).unwrap();
    /// let mut host = ponko::net::NetMessages::steam_host(&client, 3)?;
    /// host.poll()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn steam_host(client: &SteamClient, max_clients: usize) -> io::Result<Self> {
        if !(1..=249).contains(&max_clients) {
            return Err(input("max_clients must be 1..=249"));
        }
        let lobby = SteamLobby::create(client, SteamLobbyVisibility::FriendsOnly, max_clients as u32 + 1)?;
        let carrier = MessagesCarrier(lobby.api.clone());
        Ok(Self::steam_endpoint(
            client,
            Relay::host(lobby, MESSAGES, Box::new(carrier)),
            max_clients,
        ))
    }
    /// Host message traffic over Steam Networking Sockets: connections the
    /// host accepts from its lobby's members, relayed through Valve's network,
    /// with the same friends-only lobby, invites and messages as
    /// [`steam_host`](Self::steam_host). Players join with the same
    /// [`steam_join`](Self::steam_join), which follows the host's choice.
    ///
    /// [`steam_host`](Self::steam_host) (Steam Networking Messages, no
    /// connections) is the default for games among friends. Sockets suits a
    /// host that runs on its own (a headless server process signed in to a
    /// Steam account) and games that want a connection per player: the host
    /// accepts or refuses each one, and a member who leaves the lobby loses
    /// theirs at once.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if `max_clients` is outside 1..=249. Steam refusing
    /// the listen socket arrives through `poll`.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// let client = ponko::net::SteamClient::init_app(480).unwrap();
    /// let mut host = ponko::net::NetMessages::steam_sockets_host(&client, 8)?;
    /// host.poll()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn steam_sockets_host(client: &SteamClient, max_clients: usize) -> io::Result<Self> {
        if !(1..=249).contains(&max_clients) {
            return Err(input("max_clients must be 1..=249"));
        }
        let lobby = SteamLobby::create(client, SteamLobbyVisibility::FriendsOnly, max_clients as u32 + 1)?;
        let carrier = SocketsCarrier::new(Box::new(RealSockets::new(client)), true);
        Ok(Self::steam_endpoint(
            client,
            Relay::host(lobby, SOCKETS, Box::new(carrier)),
            max_clients,
        ))
    }
    /// Join a message host by lobby id. Poll until `Connected`; lobby failures
    /// are returned by `poll`. Host migration requires a new endpoint. The
    /// packets go the way the host chose: Steam Networking Messages
    /// ([`steam_host`](Self::steam_host)) or a connection over Steam
    /// Networking Sockets ([`steam_sockets_host`](Self::steam_sockets_host)).
    ///
    /// # Errors
    ///
    /// `InvalidInput` if the lobby id is zero.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # let client = ponko::net::SteamClient::init_app(480).unwrap();
    /// let net = ponko::net::NetMessages::steam_join(&client, ponko::net::SteamLobbyId::from_raw(1))?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn steam_join(client: &SteamClient, lobby: SteamLobbyId) -> io::Result<Self> {
        let lobby = SteamLobby::join(client, lobby)?;
        let api = lobby.api.clone();
        let sockets = client.clone();
        let relay = Relay::join(lobby, move |name| -> Box<dyn Carrier> {
            if name == SOCKETS {
                Box::new(SocketsCarrier::new(Box::new(RealSockets::new(&sockets)), false))
            } else {
                Box::new(MessagesCarrier(api.clone()))
            }
        });
        Ok(Self::steam_endpoint(client, relay, 1))
    }
    fn steam_endpoint(client: &SteamClient, relay: Relay, max: usize) -> Self {
        let hosting = relay.hosting;
        // Steam authenticates identities. Session membership and the normal
        // message handshake govern delivery; receiving also filters members.
        client.inner.networking_messages().session_request_callback(|r| {
            r.accept();
        });
        Self::transported(Box::new(relay), if hosting { None } else { Some(HOST) }, max)
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
    pub fn steam_peer(&self, peer: PeerId) -> Option<SteamUserId> {
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

/// In-memory Steam for tests: lobbies, Messages and Sockets, with packets
/// in memory or over the scripted network of the conformance tests.
#[cfg(test)]
pub(super) mod testing {
    use super::*;
    use crate::net::scripted::{Port, Shared};
    use std::collections::BTreeMap;

    #[derive(Default)]
    pub(super) struct Room {
        pub(super) owner: u64,
        pub(super) max: u32,
        pub(super) visibility: Option<SteamLobbyVisibility>,
        pub(super) members: Vec<u64>,
        pub(super) metadata: BTreeMap<String, String>,
    }
    #[derive(Default)]
    pub(super) struct State {
        pub(super) rooms: BTreeMap<u64, Room>,
        /// In-memory packets per receiver, Messages and Sockets alike.
        pub(super) packets: BTreeMap<u64, VecDeque<(SteamUserId, Vec<u8>)>>,
        pub(super) invites: usize,
        pub(super) largest: usize,
        /// Sockets: who listens, pending requests (host, client), open
        /// connections (lower id first), events per user, dials made.
        pub(super) listening: BTreeSet<u64>,
        pub(super) requests: BTreeSet<(u64, u64)>,
        pub(super) links: BTreeSet<(u64, u64)>,
        pub(super) events: BTreeMap<u64, VecDeque<SocketEvent>>,
        pub(super) dials: usize,
    }
    pub(super) type Steam = Arc<Mutex<State>>;

    fn link(a: u64, b: u64) -> (u64, u64) {
        (a.min(b), a.max(b))
    }

    /// Packets in memory, or over a scripted network port.
    pub(super) struct Wire {
        me: u64,
        state: Steam,
        port: Option<Mutex<Port>>,
    }
    impl Wire {
        pub(super) fn new(state: &Steam, me: u64, net: Option<&Shared>) -> Self {
            Self {
                me,
                state: state.clone(),
                port: net.map(|net| {
                    Mutex::new(Port {
                        net: net.clone(),
                        me: addr(SteamUserId::from_raw(me)),
                    })
                }),
            }
        }
        fn send(&self, peer: SteamUserId, bytes: &[u8]) -> io::Result<()> {
            if let Some(port) = &self.port {
                return port.lock().unwrap().send(addr(peer), bytes).map(drop);
            }
            let mut s = self.state.lock().unwrap();
            s.largest = s.largest.max(bytes.len());
            s.packets
                .entry(peer.raw())
                .or_default()
                .push_back((SteamUserId::from_raw(self.me), bytes.to_vec()));
            Ok(())
        }
        fn receive(&self) -> Vec<(SteamUserId, Vec<u8>)> {
            if let Some(port) = &self.port {
                let mut port = port.lock().unwrap();
                let mut buf = [0; 1500];
                let mut got = Vec::new();
                while let Ok(Some((from, n))) = port.recv(&mut buf) {
                    if let Some(peer) = steam_id(from) {
                        got.push((peer, buf[..n].to_vec()));
                    }
                }
                return got;
            }
            let mut s = self.state.lock().unwrap();
            let q = s.packets.entry(self.me).or_default();
            // Deliberate reordering, still bounded like the SDK batch.
            (0..256).filter_map(|_| q.pop_back()).collect()
        }
    }

    /// The lobby calls, and the Messages packets.
    pub(super) struct Fake {
        pub(super) me: u64,
        pub(super) state: Steam,
        pub(super) wire: Wire,
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
            complete(&result, Ok(SteamLobbyId::from_raw(self.me)), |id| self.leave(id));
        }
        fn join(&self, lobby: SteamLobbyId, result: Pending) {
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
        fn leave(&self, lobby: SteamLobbyId) {
            if let Some(r) = self.state.lock().unwrap().rooms.get_mut(&lobby.raw()) {
                r.members.retain(|&p| p != self.me);
            }
        }
        fn owner(&self, lobby: SteamLobbyId) -> SteamUserId {
            SteamUserId::from_raw(self.state.lock().unwrap().rooms[&lobby.raw()].owner)
        }
        fn members(&self, lobby: SteamLobbyId) -> Vec<SteamUserId> {
            self.state.lock().unwrap().rooms[&lobby.raw()]
                .members
                .iter()
                .map(|&p| SteamUserId::from_raw(p))
                .collect()
        }
        fn metadata(&self, lobby: SteamLobbyId, key: &str) -> Option<String> {
            self.state.lock().unwrap().rooms[&lobby.raw()]
                .metadata
                .get(key)
                .cloned()
        }
        fn set_metadata(&self, lobby: SteamLobbyId, key: &str, value: &str) -> bool {
            let mut s = self.state.lock().unwrap();
            let r = s.rooms.get_mut(&lobby.raw()).unwrap();
            if r.owner != self.me {
                return false;
            }
            r.metadata.insert(key.into(), value.into());
            true
        }
        fn invite(&self, _: SteamLobbyId) {
            self.state.lock().unwrap().invites += 1;
        }
        fn send(&self, peer: SteamUserId, bytes: &[u8]) -> io::Result<()> {
            self.wire.send(peer, bytes)
        }
        fn receive(&self) -> Vec<(SteamUserId, Vec<u8>)> {
            self.wire.receive()
        }
    }

    /// Steam Networking Sockets: requests, accepted connections, closes.
    pub(super) struct FakeSockets {
        pub(super) me: u64,
        pub(super) state: Steam,
        pub(super) wire: Wire,
    }
    impl FakeSockets {
        fn tell(s: &mut State, who: u64, event: SocketEvent) {
            s.events.entry(who).or_default().push_back(event);
        }
    }
    impl SocketsApi for FakeSockets {
        fn listen(&mut self) -> io::Result<()> {
            self.state.lock().unwrap().listening.insert(self.me);
            Ok(())
        }
        fn connect(&mut self, host: SteamUserId) -> io::Result<()> {
            let mut s = self.state.lock().unwrap();
            s.dials += 1;
            let me = SteamUserId::from_raw(self.me);
            if s.listening.contains(&host.raw()) {
                s.requests.insert((host.raw(), self.me));
                Self::tell(&mut s, host.raw(), SocketEvent::Request(me));
            } else {
                Self::tell(&mut s, self.me, SocketEvent::Closed(host));
            }
            Ok(())
        }
        fn events(&mut self) -> Vec<SocketEvent> {
            let mut s = self.state.lock().unwrap();
            s.events.remove(&self.me).map(Vec::from).unwrap_or_default()
        }
        fn accept(&mut self, peer: SteamUserId) -> bool {
            let mut s = self.state.lock().unwrap();
            if !s.requests.remove(&(self.me, peer.raw())) {
                return false;
            }
            s.links.insert(link(self.me, peer.raw()));
            Self::tell(&mut s, self.me, SocketEvent::Connected(peer));
            Self::tell(
                &mut s,
                peer.raw(),
                SocketEvent::Connected(SteamUserId::from_raw(self.me)),
            );
            true
        }
        fn close(&mut self, peer: SteamUserId) {
            let mut s = self.state.lock().unwrap();
            let refused = s.requests.remove(&(self.me, peer.raw()));
            let closed = s.links.remove(&link(self.me, peer.raw()));
            if refused || closed {
                Self::tell(&mut s, peer.raw(), SocketEvent::Closed(SteamUserId::from_raw(self.me)));
            }
        }
        fn send(&mut self, peer: SteamUserId, bytes: &[u8]) -> bool {
            let open = self.state.lock().unwrap().links.contains(&link(self.me, peer.raw()));
            open && self.wire.send(peer, bytes).is_ok()
        }
        fn receive(&mut self) -> Vec<(SteamUserId, Vec<u8>)> {
            self.wire.receive()
        }
    }

    pub(super) fn api(state: &Steam, me: u64) -> Arc<dyn SteamApi> {
        api_on(state, me, None)
    }
    fn api_on(state: &Steam, me: u64, net: Option<&Shared>) -> Arc<dyn SteamApi> {
        Arc::new(Fake {
            me,
            state: state.clone(),
            wire: Wire::new(state, me, net),
        })
    }
    pub(super) fn sockets(state: &Steam, me: u64, net: Option<&Shared>, hosting: bool) -> Box<dyn Carrier> {
        Box::new(SocketsCarrier::new(
            Box::new(FakeSockets {
                me,
                state: state.clone(),
                wire: Wire::new(state, me, net),
            }),
            hosting,
        ))
    }

    /// `1 + clients` endpoints on one fake Steam: Steam user 1 hosts
    /// with `carrier` ([`MESSAGES`] or [`SOCKETS`]) for up to `max_clients`;
    /// users 2, 3, ... join its lobby. Packets cross `net` when given.
    pub(super) fn endpoints_on(
        carrier: &'static str,
        clients: usize,
        max_clients: usize,
        net: Option<&Shared>,
    ) -> (Vec<NetMessages>, Steam) {
        let state = Steam::default();
        let nets = (1..=clients as u64 + 1)
            .map(|me| {
                let api = api_on(&state, me, net);
                if me == 1 {
                    let lobby =
                        SteamLobby::start(api.clone(), None, SteamLobbyVisibility::FriendsOnly, clients as u32 + 1);
                    let c: Box<dyn Carrier> = if carrier == SOCKETS {
                        sockets(&state, me, net, true)
                    } else {
                        Box::new(MessagesCarrier(api))
                    };
                    NetMessages::transported(Box::new(Relay::host(lobby, carrier, c)), None, max_clients)
                } else {
                    let lobby = SteamLobby::start(
                        api.clone(),
                        Some(SteamLobbyId::from_raw(1)),
                        SteamLobbyVisibility::FriendsOnly,
                        0,
                    );
                    let (state, net) = (state.clone(), net.cloned());
                    let relay = Relay::join(lobby, move |name| -> Box<dyn Carrier> {
                        if name == SOCKETS {
                            sockets(&state, me, net.as_ref(), false)
                        } else {
                            Box::new(MessagesCarrier(api.clone()))
                        }
                    });
                    NetMessages::transported(Box::new(relay), Some(HOST), 1)
                }
            })
            .collect();
        (nets, state)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{State, api, endpoints_on};
    use super::*;
    use crate::net::Lane;
    use crate::net::MAX_MESSAGE_BYTES;
    use crate::net::MessageEvent;
    use std::time::Duration;

    const CARRIERS: [&str; 2] = [MESSAGES, SOCKETS];

    fn endpoints(players: usize, carrier: &'static str) -> (Vec<NetMessages>, Arc<Mutex<State>>) {
        endpoints_on(carrier, players - 1, players - 1, None)
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
        for carrier in CARRIERS {
            let (nets, _state) = endpoints(3, carrier);
            exercise(Session::over(nets, declarations, false), 400, 200);
        }
    }
    #[test]
    fn two_and_three_peers_exchange_all_lanes_and_disconnect() {
        for carrier in CARRIERS {
            for players in [2, 3] {
                let (mut nets, state) = endpoints(players, carrier);
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
                assert_eq!(peers.len(), players - 1, "{carrier}");
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
    }
    #[test]
    fn fragmented_payload_crosses_fake_steam_relay() {
        for carrier in CARRIERS {
            let (mut nets, s) = endpoints(2, carrier);
            poll(&mut nets, 30);
            for n in &mut nets {
                n.events().for_each(drop);
            }
            for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
                let bytes = vec![73; MAX_MESSAGE_BYTES];
                nets[1].send(PeerId::HOST, lane, &bytes).unwrap();
                poll(&mut nets, 150);
                assert!(
                    nets[0].events().any(
                        |e| matches!(e, MessageEvent::Message { lane: l, bytes: b, .. } if l == lane && b == bytes)
                    ),
                    "{carrier}"
                );
            }
            assert!(s.lock().unwrap().largest <= PACKET_LIMIT);
        }
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
        let (mut nets, s) = endpoints(3, MESSAGES);
        nets.pop(); // leave one host slot free: an outsider HELLO must still be rejected
        poll(&mut nets, 30);
        for n in &mut nets {
            n.events().for_each(drop);
        }
        let mut hello = b"CRM2".to_vec();
        hello.push(1); // valid HELLO: would connect without membership filtering
        hello.extend_from_slice(&1u64.to_le_bytes());
        hello.extend_from_slice(&0u64.to_le_bytes());
        s.lock()
            .unwrap()
            .packets
            .entry(1)
            .or_default()
            .push_back((SteamUserId::from_raw(99), hello));
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
        for carrier in CARRIERS {
            let (mut nets, state) = endpoints(2, carrier);
            poll(&mut nets, 30);
            let metadata = state.lock().unwrap().rooms[&1].metadata.clone();
            assert_eq!(
                metadata.get("ponko_protocol").map(String::as_str),
                Some("ponko-messages-2")
            );
            assert_eq!(metadata.get(CARRIER_KEY).map(String::as_str), Some(carrier));
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
                metadata.insert("ponko_protocol".into(), "ponko-messages-3".into());
            }
            assert!(
                nets[1].poll_elapsed(Duration::ZERO).is_err(),
                "unknown protocol versions are rejected"
            );
        }
    }

    // Steam Networking Sockets.

    /// The packets a joiner's carrier sends before and after its connection.
    #[test]
    fn a_joiner_uses_the_carrier_the_host_published() {
        for carrier in CARRIERS {
            let (mut nets, state) = endpoints(2, carrier);
            poll(&mut nets, 30);
            let s = state.lock().unwrap();
            assert_eq!(
                !s.links.is_empty(),
                carrier == SOCKETS,
                "{carrier}: connections only for Sockets"
            );
            assert_eq!(s.dials, usize::from(carrier == SOCKETS));
        }
    }
    #[test]
    fn a_host_without_a_carrier_key_is_a_messages_host_and_unknown_carriers_are_refused() {
        let (mut nets, state) = endpoints(2, MESSAGES);
        state
            .lock()
            .unwrap()
            .rooms
            .get_mut(&1)
            .unwrap()
            .metadata
            .insert("ponko_protocol".into(), PROTOCOL.into());
        // An older host: the protocol key only.
        state
            .lock()
            .unwrap()
            .rooms
            .get_mut(&1)
            .unwrap()
            .metadata
            .remove(CARRIER_KEY);
        nets[1].poll_elapsed(Duration::ZERO).unwrap();
        poll(&mut nets, 30);
        assert!(nets[1].events().any(|e| e == MessageEvent::Connected(PeerId::HOST)));

        let (mut nets, state) = endpoints(2, MESSAGES);
        {
            let mut s = state.lock().unwrap();
            let m = &mut s.rooms.get_mut(&1).unwrap().metadata;
            m.insert("ponko_protocol".into(), PROTOCOL.into());
            m.insert(CARRIER_KEY.into(), "carrier-pigeon".into());
        }
        let e = nets[1].poll_elapsed(Duration::ZERO).unwrap_err();
        assert!(e.to_string().contains("carrier-pigeon"), "{e}");
    }
    #[test]
    fn a_sockets_host_refuses_connections_from_outside_its_lobby() {
        let (mut nets, state) = endpoints(2, SOCKETS);
        poll(&mut nets, 30);
        // Steam user 99 dials the host without joining the lobby.
        let mut outsider = super::testing::FakeSockets {
            me: 99,
            state: state.clone(),
            wire: super::testing::Wire::new(&state, 99, None),
        };
        outsider.connect(SteamUserId::from_raw(1)).unwrap();
        poll(&mut nets, 2);
        assert_eq!(outsider.events(), [SocketEvent::Closed(SteamUserId::from_raw(1))]);
        assert!(!state.lock().unwrap().links.contains(&(1, 99)));
        assert!(
            !outsider.send(SteamUserId::from_raw(1), b"hello"),
            "no connection, no packets"
        );
    }
    #[test]
    fn a_sockets_client_sends_nothing_until_the_host_accepts() {
        let state = Arc::new(Mutex::new(State::default()));
        let mut client = super::testing::sockets(&state, 2, None, false);
        let host = SteamUserId::from_raw(1);
        assert!(!client.send(host, b"early").unwrap(), "not connected: loss");
        client.maintain(&[host, SteamUserId::from_raw(2)], Some(host)).unwrap();
        assert!(!client.send(host, b"early").unwrap(), "nobody listens: the dial closed");
        let mut server = super::testing::sockets(&state, 1, None, true);
        let members = [host, SteamUserId::from_raw(2)];
        server.maintain(&members, None).unwrap(); // listens
        client.maintain(&members, Some(host)).unwrap(); // dials again after the close
        server.maintain(&members, None).unwrap(); // accepts
        client.maintain(&members, Some(host)).unwrap(); // connected
        assert!(client.send(host, b"now").unwrap());
        server.maintain(&members, None).unwrap(); // the host sees the connection open
        assert_eq!(server.receive(), [(SteamUserId::from_raw(2), b"now".to_vec())]);
        assert_eq!(state.lock().unwrap().dials, 2);
    }
    #[test]
    fn leaving_the_lobby_closes_the_sockets_connection() {
        let (mut nets, state) = endpoints(3, SOCKETS);
        poll(&mut nets, 30);
        assert_eq!(state.lock().unwrap().links.len(), 2);
        let leaver = nets.pop().unwrap();
        drop(leaver); // drops the lobby: leaves it
        poll(&mut nets, 2);
        assert_eq!(
            state.lock().unwrap().links,
            BTreeSet::from([(1, 2)]),
            "only the member left is connected"
        );
        nets[0].events().for_each(drop);
        nets[1].send(PeerId::HOST, Lane::Reliable, b"still here").unwrap();
        poll(&mut nets, 10);
        assert!(
            nets[0]
                .events()
                .any(|e| matches!(e, MessageEvent::Message { bytes, .. } if bytes == b"still here"))
        );
    }
    #[test]
    fn a_dropped_sockets_connection_is_dialed_again_and_the_session_goes_on() {
        let (mut nets, state) = endpoints(2, SOCKETS);
        poll(&mut nets, 30);
        for n in &mut nets {
            n.events().for_each(drop);
        }
        {
            // The route breaks: Steam closes the connection on both ends.
            let mut s = state.lock().unwrap();
            s.links.clear();
            s.events
                .entry(1)
                .or_default()
                .push_back(SocketEvent::Closed(SteamUserId::from_raw(2)));
            s.events
                .entry(2)
                .or_default()
                .push_back(SocketEvent::Closed(SteamUserId::from_raw(1)));
        }
        nets[1].send(PeerId::HOST, Lane::Reliable, b"after").unwrap();
        poll(&mut nets, 40);
        assert_eq!(state.lock().unwrap().dials, 2);
        assert!(
            nets[0]
                .events()
                .any(|e| matches!(e, MessageEvent::Message { bytes, .. } if bytes == b"after"))
        );
        assert!(
            !nets[1].events().any(|e| matches!(e, MessageEvent::Disconnected(..))),
            "the session survives"
        );
    }
}
