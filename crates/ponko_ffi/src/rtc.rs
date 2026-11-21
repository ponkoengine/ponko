//! WebRTC message endpoints from C (Rust `NetMessages::host_rtc`,
//! `join_rtc`), signaled through a lobby client: browser and desktop
//! players in a lobby room connect peer to peer. The endpoint is then an
//! ordinary `PonkoNetMessages`. Needs the `webrtc` feature; without it the
//! calls fail with an error naming it.

use std::ffi::c_char;

use crate::lobby::PonkoLobby;
use crate::messages::PonkoNetMessages;
use crate::{PonkoStatus, boundary, invalid};

/// The WebRTC settings from C strings: space-separated `stun:` URLs (null
/// or empty: none) and the UDP socket's address (null: any port of every
/// interface).
#[cfg(feature = "webrtc")]
fn config(stun_urls: *const c_char, bind: *const c_char) -> Result<ponko::net::rtc::RtcConfig, crate::Failure> {
    let mut config = ponko::net::rtc::RtcConfig::new();
    if !stun_urls.is_null() {
        // SAFETY: a non-null string under the caller contract.
        for url in unsafe { crate::ctx::text(stun_urls) }?.split_whitespace() {
            config = config.stun(url);
        }
    }
    if !bind.is_null() {
        // SAFETY: a non-null string under the caller contract.
        let bind = unsafe { crate::ctx::text(bind) }?;
        config = config.bind(
            bind.parse()
                .map_err(|_| invalid("bind is not an address such as 0.0.0.0:7800"))?,
        );
    }
    Ok(config)
}

#[cfg(feature = "webrtc")]
fn io(e: std::io::Error) -> crate::Failure {
    crate::invalid_owned(format!("webrtc: {e}"))
}

/// Hosts a message endpoint that takes up to `max_clients` (1..=1024)
/// clients over WebRTC, signaled through `lobby`'s room (the host is
/// normally the room's host). `stun_urls`: space-separated `stun:host:port`
/// URLs, or null. `bind`: the UDP socket's address, or null for any port.
/// Poll `lobby` every frame too. Requires the `webrtc` feature.
///
/// Ownership: on success `out_endpoint` receives an owned endpoint, the
/// caller's; free it with ponko_net_messages_free. (or give it to
/// ponko_game_messages, which takes it over).
///
/// # Safety
/// `lobby` live or null; strings null or NUL-terminated; `out_endpoint`
/// writable. On failure it is set to null.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_net_messages_rtc_host(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), 4, std::ptr::null_mut()) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_rtc_host(
    lobby: *mut PonkoLobby,
    stun_urls: *const c_char,
    bind: *const c_char,
    max_clients: usize,
    out_endpoint: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    boundary(|| {
        if out_endpoint.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: checked writable output under caller contract.
        unsafe {
            out_endpoint.write(std::ptr::null_mut());
        }
        let lobby = crate::lobby::live(lobby)?;
        #[cfg(feature = "webrtc")]
        {
            let config = config(stun_urls, bind)?;
            let net = ponko::net::NetMessages::host_rtc(lobby.client.signaling(), config, max_clients).map_err(io)?;
            // SAFETY: forwarded caller contract.
            unsafe { crate::ctx::out(out_endpoint, crate::messages::wrap(net)) }
        }
        #[cfg(not(feature = "webrtc"))]
        {
            let _ = (lobby, stun_urls, bind, max_clients);
            Err(invalid("ponko_ffi was built without the `webrtc` feature"))
        }
    })
}

/// Joins the host that lobby member `host` (the room's host, from
/// `ponko_lobby_state`) runs, over WebRTC signaled through `lobby`. Poll
/// the endpoint until it reports the host connected, and `lobby` every
/// frame too. Settings as `ponko_net_messages_rtc_host`. Requires the
/// `webrtc` feature.
///
/// Ownership: on success `out_endpoint` receives an owned endpoint, the
/// caller's; free it with ponko_net_messages_free. (or give it to
/// ponko_game_messages, which takes it over).
///
/// # Safety
/// As `ponko_net_messages_rtc_host`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_net_messages_rtc_join(std::ptr::null_mut(), 1, std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_rtc_join(
    lobby: *mut PonkoLobby,
    host: u32,
    stun_urls: *const c_char,
    bind: *const c_char,
    out_endpoint: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    boundary(|| {
        if out_endpoint.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: checked writable output under caller contract.
        unsafe {
            out_endpoint.write(std::ptr::null_mut());
        }
        let lobby = crate::lobby::live(lobby)?;
        if host == 0 {
            return Err(invalid("host member id is zero"));
        }
        #[cfg(feature = "webrtc")]
        {
            let config = config(stun_urls, bind)?;
            let net =
                ponko::net::NetMessages::join_rtc(lobby.client.signaling(), u64::from(host), config).map_err(io)?;
            // SAFETY: forwarded caller contract.
            unsafe { crate::ctx::out(out_endpoint, crate::messages::wrap(net)) }
        }
        #[cfg(not(feature = "webrtc"))]
        {
            let _ = (lobby, stun_urls, bind);
            Err(invalid("ponko_ffi was built without the `webrtc` feature"))
        }
    })
}

/// The lobby member id behind connected `peer` of a WebRTC endpoint, or
/// zero for an unknown peer or another kind of endpoint.
/// # Safety
/// Live/null endpoint; writable output.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_net_messages_rtc_peer(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_rtc_peer(
    endpoint: *mut PonkoNetMessages,
    peer: u64,
    out_member: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        let m = crate::messages::live(endpoint)?;
        #[cfg(feature = "webrtc")]
        let member = m
            .net
            .rtc_signal_peer(ponko::net::PeerId::from_bits(peer))
            .and_then(|p| u32::try_from(p).ok())
            .unwrap_or(0);
        #[cfg(not(feature = "webrtc"))]
        let member = {
            let _ = (m, peer);
            0u32
        };
        // SAFETY: forwarded caller contract.
        unsafe { crate::ctx::out(out_member, member) }
    })
}
