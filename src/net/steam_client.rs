//! Ponko's own Steam types (feature `steam`): the client, lobby ids and user
//! ids that the Steam transports take and return. They wrap the `steamworks`
//! crate, so a new `steamworks` release changes nothing in a game's code; the
//! crate itself stays reachable as [`unstable_steamworks`](super::unstable_steamworks).

use std::fmt;
use std::io;
use std::str::FromStr;

/// A started Steam client for one app (feature `steam`). Start it once, at
/// launch, and pass it to [`Net::steam_host`](super::Net::steam_host),
/// [`NetMessages::steam_host`](super::NetMessages::steam_host),
/// [`SteamLobby::create`](super::SteamLobby::create) and the other Steam
/// calls. Clones share the same client.
///
/// Ponko wraps the lobby and networking parts of Steam that its transports
/// use. For the rest (achievements, stats, rich presence, ...),
/// [`unstable_steamworks`](Self::unstable_steamworks) hands out the
/// `steamworks` client underneath; that crate's version may change in any
/// Ponko release.
///
/// ```no_run
/// let client = ponko::net::SteamClient::init_app(480)?; // 480: Valve's test app
/// println!("signed in as {}", client.user());
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Clone)]
pub struct SteamClient {
    pub(crate) inner: steamworks::Client,
}

impl SteamClient {
    /// Starts Steam for `app_id` (480 is Valve's test app "Spacewar", for
    /// development). Steam has to be running and logged in, and Valve's
    /// `libsteam_api` library has to sit next to the executable.
    ///
    /// # Errors
    ///
    /// `NotConnected` when Steam isn't running (or not logged in), another
    /// error when Steam refuses for another reason (an outdated client, an
    /// app the account doesn't own). The message says which.
    ///
    /// ```no_run
    /// let client = ponko::net::SteamClient::init_app(480)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn init_app(app_id: u32) -> io::Result<SteamClient> {
        steamworks::Client::init_app(app_id)
            .map(|inner| SteamClient { inner })
            .map_err(init_error)
    }

    /// Starts Steam for the app Steam launched this game as (or the id in a
    /// `steam_appid.txt` next to the executable, during development).
    ///
    /// # Errors
    ///
    /// As [`init_app`](Self::init_app).
    ///
    /// ```no_run
    /// let client = ponko::net::SteamClient::init()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn init() -> io::Result<SteamClient> {
        steamworks::Client::init()
            .map(|inner| SteamClient { inner })
            .map_err(init_error)
    }

    /// The app id this client was started for.
    ///
    /// ```no_run
    /// # let client = ponko::net::SteamClient::init_app(480)?;
    /// assert_eq!(client.app_id(), 480);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    #[must_use]
    pub fn app_id(&self) -> u32 {
        self.inner.utils().app_id().0
    }

    /// The signed-in player.
    ///
    /// ```no_run
    /// # let client = ponko::net::SteamClient::init_app(480)?;
    /// let me = client.user();
    /// # Ok::<(), std::io::Error>(())
    /// ```
    #[must_use]
    pub fn user(&self) -> SteamUserId {
        SteamUserId::from_unstable(self.inner.user().steam_id())
    }

    /// Runs Steam's pending callbacks. The Steam transports and
    /// [`SteamLobby::poll`](super::SteamLobby::poll) do this themselves;
    /// call it each frame only when nothing else does and you use
    /// [`unstable_steamworks`](Self::unstable_steamworks) callbacks.
    ///
    /// ```no_run
    /// # let client = ponko::net::SteamClient::init_app(480)?;
    /// client.run_callbacks();
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn run_callbacks(&self) {
        self.inner.run_callbacks();
    }

    /// The `steamworks` client underneath, for the parts of Steam that Ponko
    /// doesn't wrap (achievements, stats, rich presence, ...).
    ///
    /// **Unstable**: its type is the `steamworks` crate's
    /// ([`unstable_steamworks`](super::unstable_steamworks)), whose version
    /// may change in any Ponko release. Ids pass between the two with `From`
    /// (as unstable as this method): [`SteamLobbyId`] and `LobbyId`,
    /// [`SteamUserId`] and `SteamId`.
    ///
    /// ```no_run
    /// # let client = ponko::net::SteamClient::init_app(480)?;
    /// # let lobby = ponko::net::SteamLobbyId::from_raw(1);
    /// let stats = client.unstable_steamworks().user_stats();
    /// let owner: ponko::net::SteamUserId = client.unstable_steamworks().matchmaking().lobby_owner(lobby.into()).into();
    /// # Ok::<(), std::io::Error>(())
    /// ```
    #[must_use]
    pub fn unstable_steamworks(&self) -> &steamworks::Client {
        &self.inner
    }

    /// Wraps a `steamworks` client that the game started itself.
    ///
    /// **Unstable**, as [`unstable_steamworks`](Self::unstable_steamworks).
    ///
    /// ```no_run
    /// let raw = ponko::net::unstable_steamworks::Client::init_app(480).unwrap();
    /// let client = ponko::net::SteamClient::from_unstable_steamworks(raw);
    /// ```
    #[must_use]
    pub fn from_unstable_steamworks(client: steamworks::Client) -> SteamClient {
        SteamClient { inner: client }
    }
}

impl fmt::Debug for SteamClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamClient").field("app_id", &self.app_id()).finish()
    }
}

fn init_error(e: steamworks::SteamAPIInitError) -> io::Error {
    let kind = match e {
        steamworks::SteamAPIInitError::NoSteamClient(_) => io::ErrorKind::NotConnected,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(
        kind,
        format!("could not start Steam (is it running and logged in?): {e}"),
    )
}

macro_rules! steam_id {
    ($(#[$doc:meta])* $name:ident, $raw:ty) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            /// The id from its 64-bit number (as Steam shows it, and as the
            /// C API passes it).
            #[must_use]
            pub const fn from_raw(raw: u64) -> Self {
                Self(raw)
            }
            /// Its 64-bit number.
            #[must_use]
            pub const fn raw(self) -> u64 {
                self.0
            }
            pub(super) fn from_unstable(id: $raw) -> Self {
                Self(id.raw())
            }
            pub(super) fn to_unstable(self) -> $raw {
                <$raw>::from_raw(self.0)
            }
        }

        /// The `steamworks` crate's id, for calls through
        /// [`SteamClient::unstable_steamworks`].
        ///
        /// **Unstable**: the target is the `steamworks` crate's type
        /// ([`unstable_steamworks`](super::unstable_steamworks)), whose
        /// version may change in any Ponko release.
        impl From<$name> for $raw {
            fn from(id: $name) -> $raw {
                id.to_unstable()
            }
        }

        /// Ponko's id, from one the `steamworks` crate handed out.
        ///
        /// **Unstable**, as the conversion the other way.
        impl From<$raw> for $name {
            fn from(id: $raw) -> $name {
                $name::from_unstable(id)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }

        /// The 64-bit number.
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        /// Parses the 64-bit number, as [`Display`](fmt::Display) writes it.
        impl FromStr for $name {
            type Err = std::num::ParseIntError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }
    };
}

steam_id! {
    /// A Steam lobby (feature `steam`): what a host shows and a friend
    /// joins with, from an invite ([`steam_lobby_from_args`](super::steam_lobby_from_args),
    /// [`steam_wait_for_invite`](super::steam_wait_for_invite)) or typed in.
    ///
    /// ```
    /// use ponko::net::SteamLobbyId;
    /// let lobby: SteamLobbyId = "109775241090369130".parse().unwrap();
    /// assert_eq!(lobby.raw(), 109_775_241_090_369_130);
    /// assert_eq!(lobby.to_string(), "109775241090369130");
    /// assert_eq!(SteamLobbyId::from_raw(lobby.raw()), lobby);
    /// ```
    SteamLobbyId, steamworks::LobbyId
}

steam_id! {
    /// A Steam user (feature `steam`): a lobby member
    /// ([`SteamLobby::members`](super::SteamLobby::members)), the peer behind a
    /// message connection ([`NetMessages::steam_peer`](super::NetMessages::steam_peer))
    /// or the signed-in player ([`SteamClient::user`]). Steam authenticates it.
    ///
    /// ```
    /// use ponko::net::SteamUserId;
    /// let user = SteamUserId::from_raw(76_561_197_960_287_930);
    /// assert_eq!(user.to_string().parse::<SteamUserId>(), Ok(user));
    /// ```
    SteamUserId, steamworks::SteamId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_steamworks_and_text() {
        let lobby = SteamLobbyId::from_raw(109_775_241_090_369_130);
        assert_eq!(SteamLobbyId::from_unstable(lobby.to_unstable()), lobby);
        assert_eq!(lobby.to_unstable().raw(), lobby.raw());
        assert_eq!(format!("{lobby:?}"), "SteamLobbyId(109775241090369130)");
        let user = SteamUserId::from_raw(76_561_197_960_287_930);
        assert_eq!(SteamUserId::from_unstable(user.to_unstable()), user);
        assert_eq!("76561197960287930".parse(), Ok(user));
        assert!("lobby".parse::<SteamLobbyId>().is_err());
    }

    #[test]
    fn ids_convert_to_and_from_the_steamworks_ids() {
        let lobby = SteamLobbyId::from_raw(109_775_241_090_369_130);
        let raw: steamworks::LobbyId = lobby.into();
        assert_eq!(raw.raw(), lobby.raw());
        assert_eq!(SteamLobbyId::from(raw), lobby);
        let user = SteamUserId::from_raw(76_561_197_960_287_930);
        let raw = steamworks::SteamId::from(user);
        assert_eq!(raw.raw(), user.raw());
        assert_eq!(SteamUserId::from(raw), user);
    }

    #[test]
    fn init_errors_say_steam_is_missing() {
        let e = init_error(steamworks::SteamAPIInitError::NoSteamClient("no client".into()));
        assert_eq!(e.kind(), io::ErrorKind::NotConnected);
        assert!(e.to_string().contains("is it running"), "{e}");
        let e = init_error(steamworks::SteamAPIInitError::VersionMismatch("old".into()));
        assert_eq!(e.kind(), io::ErrorKind::Other);
    }
}
