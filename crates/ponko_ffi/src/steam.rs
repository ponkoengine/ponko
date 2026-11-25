//! Lockstep over Steam from C (feature `steam`, off by default: it links
//! Valve's libsteam_api, which ships next to the game). Without the
//! feature every call fails, saying why.
//!
//! The Steam client is started once per process, by the first call that
//! takes an `app_id`.

use crate::ctx::PonkoCtx;
#[cfg(feature = "steam")]
use crate::{Failure, ctx::ctx_access, ctx::out, game::build, invalid_owned};
use crate::{PonkoGame, PonkoStatus, boundary, invalid};

#[cfg(feature = "steam")]
pub(crate) mod client {
    use super::*;
    use std::sync::Mutex;

    static CLIENT: Mutex<Option<(u32, ponko::net::SteamClient)>> = Mutex::new(None);

    /// The process's Steam client for `app_id`, started on first use.
    pub(crate) fn get(app_id: u32) -> Result<ponko::net::SteamClient, Failure> {
        let mut c = CLIENT.lock().map_err(|_| invalid("steam client lock poisoned"))?;
        if let Some((id, client)) = c.as_ref() {
            if *id != app_id {
                return Err(invalid_owned(format!(
                    "Steam already started for app {id}, not {app_id}"
                )));
            }
            return Ok(client.clone());
        }
        let client = ponko::net::SteamClient::init_app(app_id).map_err(|e| invalid_owned(e.to_string()))?;
        *c = Some((app_id, client.clone()));
        Ok(client)
    }
}

#[cfg(not(feature = "steam"))]
fn off<T>() -> Result<T, crate::Failure> {
    Err(invalid("ponko_ffi was built without the `steam` feature"))
}

/// Hosts a lockstep session for `players` (2-8) over Steam: starts Steam
/// for `app_id` (480 is Valve's test app) and creates a friends-only lobby
/// that friends join from the overlay (`ponko_ctx_steam_invite`). Packets
/// go through Steam's relays: no addresses, no port forwarding. Delay and
/// checks as in `ponko_game_lockstep_host`.
/// # Safety
/// Live game.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_lockstep_steam_host(std::ptr::null_mut(), 480, 2, 3, 60) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_lockstep_steam_host(
    game: *mut PonkoGame,
    app_id: u32,
    players: u32,
    input_delay: u32,
    checksum_every: u32,
) -> PonkoStatus {
    let mut net: Option<ponko::net::Net> = None;
    let status = boundary(|| {
        if game.is_null() {
            return Err(invalid("game is null"));
        }
        if !(2..=ponko::net::MAX_PLAYERS as u32).contains(&players) {
            return Err(invalid("2 to 8 players"));
        }
        if input_delay > 64 {
            return Err(invalid("input delay is at most 64 ticks"));
        }
        #[cfg(feature = "steam")]
        {
            let client = client::get(app_id)?;
            net = Some(
                ponko::net::Net::steam_host(&client, players as usize)
                    .input_delay(input_delay)
                    .checksum_every(checksum_every),
            );
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, checksum_every, &mut net);
            off()
        }
    });
    #[cfg(feature = "steam")]
    if let Some(net) = net {
        // SAFETY: forwarded caller contract.
        return unsafe { build(game, |g| (g.lockstep(net), Ok(()))) };
    }
    status
}

/// Joins the Steam lobby `lobby` (from an invite: `ponko_steam_lobby_from_args`
/// or `ponko_steam_wait_for_invite`), starting Steam for `app_id`.
/// # Safety
/// Live game.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_lockstep_steam_join(std::ptr::null_mut(), 480, 1) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_lockstep_steam_join(game: *mut PonkoGame, app_id: u32, lobby: u64) -> PonkoStatus {
    let mut net: Option<ponko::net::Net> = None;
    let status = boundary(|| {
        if game.is_null() {
            return Err(invalid("game is null"));
        }
        #[cfg(feature = "steam")]
        {
            let client = client::get(app_id)?;
            let lobby = ponko::net::SteamLobbyId::from_raw(lobby);
            net = Some(ponko::net::Net::steam_join(&client, lobby));
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, lobby, &mut net);
            off()
        }
    });
    #[cfg(feature = "steam")]
    if let Some(net) = net {
        // SAFETY: forwarded caller contract.
        return unsafe { build(game, |g| (g.lockstep(net), Ok(()))) };
    }
    status
}

/// The lobby of the Steam invite this game was launched with (Steam passes
/// `+connect_lobby <id>` when a friend's invite is accepted while the game
/// isn't running), or 0.
/// # Safety
/// Writable output.
/// ```
/// let mut lobby = 7;
/// let status = unsafe { ponko_ffi::ponko_steam_lobby_from_args(&mut lobby) };
/// # #[cfg(feature = "steam")]
/// assert_eq!((status as u32, lobby), (0, 0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_from_args(out_lobby: *mut u64) -> PonkoStatus {
    boundary(|| {
        if out_lobby.is_null() {
            return Err(invalid("output is null"));
        }
        #[cfg(feature = "steam")]
        {
            let lobby = ponko::net::steam_lobby_from_args().map_or(0, |l| l.raw());
            // SAFETY: checked non-null; caller supplies writable storage.
            unsafe { out(out_lobby, lobby) }
        }
        #[cfg(not(feature = "steam"))]
        off()
    })
}

/// Waits up to `timeout_ms` for the player to accept a friend's Steam
/// invite while the game runs; writes the lobby, or 0 if none came.
/// Starts Steam for `app_id`.
/// # Safety
/// Writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_wait_for_invite(480, 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_wait_for_invite(app_id: u32, timeout_ms: u32, out_lobby: *mut u64) -> PonkoStatus {
    boundary(|| {
        if out_lobby.is_null() {
            return Err(invalid("output is null"));
        }
        #[cfg(feature = "steam")]
        {
            let client = client::get(app_id)?;
            let wait = std::time::Duration::from_millis(timeout_ms.into());
            let lobby = ponko::net::steam_wait_for_invite(&client, wait).map_or(0, |l| l.raw());
            // SAFETY: checked non-null; caller supplies writable storage.
            unsafe { out(out_lobby, lobby) }
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, timeout_ms);
            off()
        }
    })
}

/// This session's Steam lobby, or 0 (not created yet, or not a Steam session).
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_steam_lobby(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_steam_lobby(ctx: *mut PonkoCtx, out_lobby: *mut u64) -> PonkoStatus {
    boundary(|| {
        if out_lobby.is_null() {
            return Err(invalid("output is null"));
        }
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            let lobby = ctx_access(ctx, |c| Ok(c.net.steam_lobby().map_or(0, |l| l.raw())))?;
            out(out_lobby, lobby)
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = ctx;
            off()
        }
    })
}

/// Opens the Steam overlay's dialog to invite friends to this session's
/// lobby. Does nothing until the lobby exists.
/// # Safety
/// Live ctx.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_steam_invite(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_steam_invite(ctx: *mut PonkoCtx) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                c.net.steam_invite();
                Ok(())
            })
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = ctx;
            off()
        }
    })
}
