//! General Steam lobbies and message endpoint lobby access.
#[cfg(feature = "steam")]
use crate::ctx::{out, text};
use crate::{PonkoStatus, boundary, invalid};
use std::ffi::c_char;

/// Owned lobby handle. Free once with `ponko_steam_lobby_free`.
pub struct PonkoSteamLobby {
    #[cfg(feature = "steam")]
    lobby: ponko::net::SteamLobby,
}
#[cfg(feature = "steam")]
fn live<'a>(p: *mut PonkoSteamLobby) -> Result<&'a mut ponko::net::SteamLobby, crate::Failure> {
    // SAFETY: callers supply null or a live lobby handle.
    unsafe { p.as_mut() }
        .map(|p| &mut p.lobby)
        .ok_or_else(|| invalid("lobby is null"))
}

/// Create an asynchronous Steam lobby. Visibility: 0 friends-only, 1 public, 2 private.
///
/// Ownership: on success `output` receives an owned lobby, the caller's; free
/// it with ponko_steam_lobby_free.
///
/// # Safety
/// Writable output; set to null on failure.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_create(480, 0, 2, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_create(
    app_id: u32,
    visibility: u32,
    max_members: u32,
    output: *mut *mut PonkoSteamLobby,
) -> PonkoStatus {
    boundary(|| {
        if output.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            output.write(std::ptr::null_mut());
        }
        if visibility > 2 || !(1..=250).contains(&max_members) {
            return Err(invalid("invalid visibility or member count"));
        }
        #[cfg(feature = "steam")]
        {
            let client = crate::steam::client::get(app_id)?;
            let lobby = ponko::net::SteamLobby::create(
                &client,
                match visibility {
                    0 => ponko::net::SteamLobbyVisibility::FriendsOnly,
                    1 => ponko::net::SteamLobbyVisibility::Public,
                    _ => ponko::net::SteamLobbyVisibility::Private,
                },
                max_members,
            )?;
            // SAFETY: forwarded caller contract.
            unsafe { out(output, Box::into_raw(Box::new(PonkoSteamLobby { lobby }))) }
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, visibility, max_members);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Join an asynchronous Steam lobby. Visibility: 0 friends-only, 1 public, 2 private.
///
/// Ownership: on success `output` receives an owned lobby, the caller's; free
/// it with ponko_steam_lobby_free.
///
/// # Safety
/// Writable output; set to null on failure.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_join(480, 1, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_join(
    app_id: u32,
    id: u64,
    output: *mut *mut PonkoSteamLobby,
) -> PonkoStatus {
    boundary(|| {
        if output.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            output.write(std::ptr::null_mut());
        }
        if id == 0 {
            return Err(invalid("lobby id is zero"));
        }
        #[cfg(feature = "steam")]
        {
            let client = crate::steam::client::get(app_id)?;
            let lobby = ponko::net::SteamLobby::join(&client, ponko::net::steam::LobbyId::from_raw(id))?;
            // SAFETY: forwarded caller contract.
            unsafe { out(output, Box::into_raw(Box::new(PonkoSteamLobby { lobby }))) }
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, id);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Free an owned lobby, leaving it. Null is allowed.
/// # Safety
/// Null or a live owned lobby, never used again.
/// ```
/// unsafe { ponko_ffi::ponko_steam_lobby_free(std::ptr::null_mut()); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_free(lobby: *mut PonkoSteamLobby) {
    if !lobby.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(lobby) });
    }
}

/// Lobby poll. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_poll(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_poll(lobby: *mut PonkoSteamLobby) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        {
            live(lobby)?.poll()?;
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = lobby;
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Lobby leave. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_leave(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_leave(lobby: *mut PonkoSteamLobby) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        {
            live(lobby)?.leave()?;
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = lobby;
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Lobby id. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_id(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_id(lobby: *mut PonkoSteamLobby, output: *mut u64) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            out(output, live(lobby)?.id().map_or(0, |id| id.raw()))
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (lobby, output);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Lobby invite. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_invite(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_invite(lobby: *mut PonkoSteamLobby) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        {
            live(lobby)?.invite()?;
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = lobby;
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Lobby set metadata. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_set_metadata(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_set_metadata(
    lobby: *mut PonkoSteamLobby,
    key: *const c_char,
    value: *const c_char,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            live(lobby)?.set_metadata(text(key)?, text(value)?)?;
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (lobby, key, value);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Lobby metadata. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_metadata(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_metadata(
    lobby: *mut PonkoSteamLobby,
    key: *const c_char,
    buffer: *mut c_char,
    capacity: usize,
    needed: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            copy_text(live(lobby)?.metadata(text(key)?), buffer, capacity, needed)
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (lobby, key, buffer, capacity, needed);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Lobby members. Poll drives callbacks; id is zero until ready.
/// For members/metadata, `needed` reports required capacity; null buffer with
/// zero capacity queries size. Metadata includes NUL; missing key needs zero.
/// Insufficient capacity returns an error without partial output.
/// # Safety
/// Live/null lobby, readable NUL-terminated strings, writable outputs and buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_lobby_members(std::ptr::null_mut(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_lobby_members(
    lobby: *mut PonkoSteamLobby,
    members: *mut u64,
    capacity: usize,
    needed: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            copy_members(live(lobby)?.members(), members, capacity, needed)
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (lobby, members, capacity, needed);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

#[cfg(feature = "steam")]
unsafe fn copy_members(
    values: Vec<ponko::net::steam::SteamId>,
    buffer: *mut u64,
    capacity: usize,
    needed: *mut usize,
) -> Result<(), crate::Failure> {
    // SAFETY: forwarded writable output contract.
    unsafe {
        out(needed, values.len())?;
    }
    if capacity == 0 {
        return Ok(());
    }
    if buffer.is_null() || capacity < values.len() {
        return Err(invalid("member buffer too small or null"));
    }
    for (i, id) in values.into_iter().enumerate() {
        // SAFETY: capacity checked; caller supplies writable elements.
        unsafe {
            buffer.add(i).write(id.raw());
        }
    }
    Ok(())
}
#[cfg(feature = "steam")]
unsafe fn copy_text(
    value: Option<String>,
    buffer: *mut c_char,
    capacity: usize,
    needed: *mut usize,
) -> Result<(), crate::Failure> {
    let value = value.map(|s| std::ffi::CString::new(s.replace('\0', " ")).unwrap_or_default());
    let bytes = value.as_ref().map_or(&[][..], |v| v.as_bytes_with_nul());
    // SAFETY: forwarded writable output contract.
    unsafe {
        out(needed, bytes.len())?;
    }
    if capacity == 0 {
        return Ok(());
    }
    if buffer.is_null() || capacity < bytes.len() {
        return Err(invalid("metadata buffer too small or null"));
    }
    // SAFETY: checked capacity, nonoverlapping owned input and caller buffer.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast(), bytes.len());
    }
    Ok(())
}

/// Steam message endpoint lobby id; errors for non-Steam endpoints.
/// Buffer semantics match `ponko_steam_lobby_id`.
/// # Safety
/// Live/null endpoint, readable NUL strings and writable outputs/buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_lobby_id(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_lobby_id(
    endpoint: *mut crate::PonkoNetMessages,
    output: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            out(output, endpoint_lobby(endpoint)?.id().map_or(0, |id| id.raw()))
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (endpoint, output);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Steam message endpoint lobby invite; errors for non-Steam endpoints.
/// Buffer semantics match `ponko_steam_lobby_invite`.
/// # Safety
/// Live/null endpoint, readable NUL strings and writable outputs/buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_lobby_invite(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_lobby_invite(endpoint: *mut crate::PonkoNetMessages) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        {
            endpoint_lobby(endpoint)?.invite()?;
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = endpoint;
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Steam message endpoint lobby set metadata; errors for non-Steam endpoints.
/// Buffer semantics match `ponko_steam_lobby_set_metadata`.
/// # Safety
/// Live/null endpoint, readable NUL strings and writable outputs/buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_lobby_set_metadata(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_lobby_set_metadata(
    endpoint: *mut crate::PonkoNetMessages,
    key: *const c_char,
    value: *const c_char,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            endpoint_lobby(endpoint)?.set_metadata(text(key)?, text(value)?)?;
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (endpoint, key, value);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Steam message endpoint lobby metadata; errors for non-Steam endpoints.
/// Buffer semantics match `ponko_steam_lobby_metadata`.
/// # Safety
/// Live/null endpoint, readable NUL strings and writable outputs/buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_lobby_metadata(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_lobby_metadata(
    endpoint: *mut crate::PonkoNetMessages,
    key: *const c_char,
    buffer: *mut c_char,
    capacity: usize,
    needed: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            copy_text(endpoint_lobby(endpoint)?.metadata(text(key)?), buffer, capacity, needed)
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (endpoint, key, buffer, capacity, needed);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Steam message endpoint lobby members; errors for non-Steam endpoints.
/// Buffer semantics match `ponko_steam_lobby_members`.
/// # Safety
/// Live/null endpoint, readable NUL strings and writable outputs/buffers of `capacity` elements.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_lobby_members(std::ptr::null_mut(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_lobby_members(
    endpoint: *mut crate::PonkoNetMessages,
    members: *mut u64,
    capacity: usize,
    needed: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        #[cfg(feature = "steam")]
        // SAFETY: forwarded caller contract.
        unsafe {
            copy_members(endpoint_lobby(endpoint)?.members(), members, capacity, needed)
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (endpoint, members, capacity, needed);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

#[cfg(feature = "steam")]
fn endpoint_lobby<'a>(endpoint: *mut crate::PonkoNetMessages) -> Result<&'a ponko::net::SteamLobby, crate::Failure> {
    // SAFETY: caller supplies null or live endpoint.
    let endpoint = unsafe { endpoint.as_ref() }.ok_or_else(|| invalid("endpoint is null"))?;
    endpoint
        .net
        .steam_lobby()
        .ok_or_else(|| invalid("not a Steam endpoint"))
}

#[cfg(all(test, feature = "steam"))]
mod tests {
    use super::*;
    #[test]
    fn lobby_output_buffers_report_size_and_never_partially_write() {
        let mut needed = 99;
        let mut text_buffer = [0i8; 4];
        let mut members = [0u64; 2];
        // SAFETY: all arrays and scalar outputs below are live and sized as supplied.
        unsafe {
            assert!(copy_text(Some("abc".into()), std::ptr::null_mut(), 0, &mut needed).is_ok());
            assert_eq!(needed, 4);
            assert!(copy_text(Some("abc".into()), text_buffer.as_mut_ptr(), 3, &mut needed).is_err());
            assert_eq!(text_buffer, [0; 4]);
            assert!(copy_text(Some("abc".into()), text_buffer.as_mut_ptr(), 4, &mut needed).is_ok());
            assert_eq!(text_buffer, [97, 98, 99, 0]);
            assert!(copy_text(None, text_buffer.as_mut_ptr(), 4, &mut needed).is_ok());
            assert_eq!(needed, 0);
            let ids = vec![
                ponko::net::steam::SteamId::from_raw(7),
                ponko::net::steam::SteamId::from_raw(9),
            ];
            assert!(copy_members(ids.clone(), members.as_mut_ptr(), 1, &mut needed).is_err());
            assert_eq!(members, [0; 2]);
            assert!(copy_members(ids, members.as_mut_ptr(), 2, &mut needed).is_ok());
            assert_eq!(members, [7, 9]);
        }
    }
}
