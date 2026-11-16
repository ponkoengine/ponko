//! Lockstep over Steam (feature `steam`): Steam's relays carry the packets
//! (no port forwarding, no IP addresses), and players find each other
//! through a friends-only Steam lobby: invites from the overlay, or the
//! lobby id.

use std::collections::VecDeque;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
use std::sync::{Arc, Mutex};

use steamworks::networking_messages::NetworkingMessages;
use steamworks::networking_types::{NetworkingIdentity, SendFlags};
use steamworks::{Client, LobbyId, LobbyType, SteamId};

use super::{Net, Role, Session, Transport};

/// Steam ids travel inside the session as addresses in a private IPv6 range,
/// so the lockstep code is the same for UDP and Steam.
const PREFIX: [u16; 4] = [0xfd63, 0x7261, 0x6262, 0x7900];
/// "The lobby owner", before we know who that is.
pub(super) const HOST: SocketAddr = SocketAddr::V6(SocketAddrV6::new(
    Ipv6Addr::new(0xfd63, 0x7261, 0x6262, 0x7900, 0, 0, 0, 0),
    1,
    0,
    0,
));

pub(super) fn addr(id: SteamId) -> SocketAddr {
    let b = id.raw();
    let ip = Ipv6Addr::new(
        PREFIX[0],
        PREFIX[1],
        PREFIX[2],
        PREFIX[3],
        (b >> 48) as u16,
        (b >> 32) as u16,
        (b >> 16) as u16,
        b as u16,
    );
    SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, 0))
}
pub(super) fn steam_id(a: SocketAddr) -> Option<SteamId> {
    let SocketAddr::V6(v6) = a else { return None };
    let s = v6.ip().segments();
    if s[..4] != PREFIX || a == HOST {
        return None;
    }
    Some(SteamId::from_raw(
        u64::from(s[4]) << 48 | u64::from(s[5]) << 32 | u64::from(s[6]) << 16 | u64::from(s[7]),
    ))
}

/// What Steam's callbacks tell us, from whichever thread runs them.
#[derive(Default)]
struct Lobby {
    id: Option<LobbyId>,
    error: Option<String>,
}

struct Steam {
    client: Client,
    messages: NetworkingMessages,
    me: SocketAddr,
    lobby: Arc<Mutex<Lobby>>,
    /// The lobby owner, once joined (clients only).
    owner: Option<SteamId>,
    hosting: bool,
    inbox: VecDeque<(SocketAddr, Vec<u8>)>,
    reported: bool,
}

impl Steam {
    fn new(client: &Client, hosting: bool) -> Steam {
        let messages = client.networking_messages();
        // Anyone may start a session; the lockstep handshake (seed, player
        // count) decides who gets in.
        messages.session_request_callback(|request| {
            request.accept();
        });
        Steam {
            client: client.clone(),
            messages,
            me: addr(client.user().steam_id()),
            lobby: Arc::default(),
            owner: None,
            hosting,
            inbox: VecDeque::new(),
            reported: false,
        }
    }

    fn lobby(&self) -> Option<LobbyId> {
        self.lobby.lock().expect("lobby").id
    }
}

impl Transport for Steam {
    fn send(&mut self, to: SocketAddr, data: &[u8]) {
        let target = if to == HOST { self.owner } else { steam_id(to) };
        let Some(id) = target else { return }; // not joined yet; the hello is resent
        let flags = SendFlags::UNRELIABLE_NO_NAGLE | SendFlags::AUTO_RESTART_BROKEN_SESSION;
        // Lost packets are resent anyway.
        let _ = self
            .messages
            .send_message_to_user(NetworkingIdentity::new_steam_id(id), flags, data, 0);
    }

    fn recv(&mut self, buf: &mut [u8]) -> Option<(SocketAddr, usize)> {
        if self.inbox.is_empty() {
            self.client.run_callbacks();
            if !self.hosting
                && self.owner.is_none()
                && let Some(lobby) = self.lobby()
            {
                let owner = self.client.matchmaking().lobby_owner(lobby);
                if owner.raw() != 0 {
                    self.owner = Some(owner);
                }
            }
            for m in self.messages.receive_messages_on_channel(0, 64) {
                let Some(from) = m.identity_peer().steam_id() else {
                    continue;
                };
                // The session knows the host only as HOST.
                let from = if Some(from) == self.owner { HOST } else { addr(from) };
                self.inbox.push_back((from, m.data().to_vec()));
            }
        }
        let (from, data) = self.inbox.pop_front()?;
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        Some((from, n))
    }

    fn local_addr(&self) -> SocketAddr {
        self.me
    }

    fn error(&mut self) -> Option<String> {
        if self.reported {
            return None;
        }
        let e = self.lobby.lock().expect("lobby").error.take();
        self.reported = e.is_some();
        e
    }

    fn steam(&self) -> Option<(&Client, Option<LobbyId>)> {
        Some((&self.client, self.lobby()))
    }
}

impl Drop for Steam {
    fn drop(&mut self) {
        if let Some(lobby) = self.lobby() {
            self.client.matchmaking().leave_lobby(lobby);
        }
    }
}

impl Net {
    /// Hosts a session for `players` (2–8) on Steam: creates a friends-only
    /// lobby that friends join from the Steam overlay ("Join game", or an
    /// invite from [`steam_invite`](Self::steam_invite)). Packets go through
    /// Steam's relays: no port forwarding, no addresses to type.
    ///
    /// `client` is your game's [`steamworks::Client`] (also for
    /// achievements and the rest of Steam). Feature `steam`.
    ///
    /// ```no_run
    /// use ponko::prelude::*;
    /// let client = steamworks::Client::init_app(480).expect("is Steam running?"); // 480: Valve's test app
    /// Game::new("RTS", 1280, 720)
    ///     .lockstep(ponko::net::Net::steam_host(&client, 2))
    ///     .run(|_| {});
    /// ```
    ///
    /// # Panics
    ///
    /// Unless `players` is 2 to 8.
    #[must_use]
    pub fn steam_host(client: &Client, players: usize) -> Net {
        assert!(
            (2..=super::MAX_PLAYERS).contains(&players),
            "Steam: expected 2 to 8 players; pass a player count in that range"
        );
        let steam = Steam::new(client, true);
        let lobby = steam.lobby.clone();
        client
            .matchmaking()
            .create_lobby(LobbyType::FriendsOnly, players as u32, move |result| {
                let mut l = lobby.lock().expect("lobby");
                match result {
                    Ok(id) => l.id = Some(id),
                    Err(e) => l.error = Some(format!("could not create a Steam lobby: {e}")),
                }
            });
        Net::with(Session::new(Box::new(steam), Role::Host), 0, players)
    }

    /// Joins the Steam lobby `lobby` (its host's session): from an invite
    /// the game was launched with ([`steam_lobby_from_args`]), or a lobby id
    /// shown by the host. Feature `steam`.
    ///
    /// ```no_run
    /// use ponko::prelude::*;
    /// let client = steamworks::Client::init_app(480).expect("is Steam running?");
    /// let net = match ponko::net::steam_lobby_from_args() {
    ///     Some(lobby) => ponko::net::Net::steam_join(&client, lobby), // accepted an invite
    ///     None => ponko::net::Net::steam_host(&client, 2),
    /// };
    /// Game::new("RTS", 1280, 720).lockstep(net).run(|_| {});
    /// ```
    #[must_use]
    pub fn steam_join(client: &Client, lobby: LobbyId) -> Net {
        let steam = Steam::new(client, false);
        let joined = steam.lobby.clone();
        client.matchmaking().join_lobby(lobby, move |result| {
            let mut l = joined.lock().expect("lobby");
            match result {
                Ok(id) => l.id = Some(id),
                Err(()) => l.error = Some("could not join the Steam lobby (full, closed or not a friend's)".into()),
            }
        });
        Net::with(Session::new(Box::new(steam), Role::Client(HOST)), 0, 0)
    }

    /// The Steam lobby of this session, once created or joined (`None`
    /// for other sessions).
    #[must_use]
    pub fn steam_lobby(&self) -> Option<LobbyId> {
        self.session.as_ref()?.transport.steam()?.1
    }

    /// Opens the Steam overlay's dialog to invite friends to this
    /// session's lobby. Does nothing until the lobby exists, or without Steam.
    pub fn steam_invite(&self) {
        if let Some((client, Some(lobby))) = self.session.as_ref().and_then(|s| s.transport.steam()) {
            client.friends().activate_invite_dialog(lobby);
        }
    }
}

/// The lobby of a Steam invite this game was launched with (Steam starts
/// the game with `+connect_lobby <id>` when a friend accepts an invite
/// while it isn't running). Feature `steam`.
///
/// ```
/// // Not launched from an invite here.
/// assert!(ponko::net::steam_lobby_from_args().is_none());
/// ```
pub fn steam_lobby_from_args() -> Option<LobbyId> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "+connect_lobby")?;
    args.get(i + 1)?.parse().ok().map(LobbyId::from_raw)
}

/// Waits up to `timeout` for the player to accept a friend's Steam invite
/// to a lobby, while the game is running (Steam tells a running game
/// instead of starting it). For a "join a friend" screen before the
/// game starts; [`steam_lobby_from_args`] covers invites accepted while it
/// wasn't running. Feature `steam`.
///
/// ```no_run
/// use ponko::prelude::*;
/// let client = steamworks::Client::init_app(480).expect("is Steam running?");
/// println!("accept your friend's invite in Steam...");
/// if let Some(lobby) = ponko::net::steam_wait_for_invite(&client, std::time::Duration::from_secs(120)) {
///     Game::new("RTS", 1280, 720).lockstep(ponko::net::Net::steam_join(&client, lobby)).run(|_| {});
/// }
/// ```
#[must_use]
pub fn steam_wait_for_invite(client: &Client, timeout: std::time::Duration) -> Option<LobbyId> {
    let accepted = Arc::new(Mutex::new(None));
    let seen = accepted.clone();
    let _listening = client.register_callback(move |r: steamworks::GameLobbyJoinRequested| {
        *seen.lock().expect("invite") = Some(r.lobby_steam_id);
    });
    let start = crate::perf::Instant::now();
    while start.elapsed() < timeout {
        client.run_callbacks();
        if let Some(lobby) = *accepted.lock().expect("invite") {
            return Some(lobby);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_ids_round_trip_through_addresses() {
        let id = SteamId::from_raw(76_561_197_960_287_930);
        assert_eq!(steam_id(addr(id)), Some(id));
        assert_eq!(steam_id(HOST), None, "the host placeholder is no one");
        assert_eq!(steam_id("127.0.0.1:7777".parse().unwrap()), None);
    }
}
