//! Argument validation never initializes the real Steam API.
use ponko_ffi::*;
#[test]
fn steam_message_and_lobby_arguments_fail_before_initialization() {
    let mut endpoint = std::ptr::null_mut();
    let mut lobby = std::ptr::null_mut();
    // SAFETY: writable outputs and null handles are permitted by these APIs.
    unsafe {
        assert_eq!(
            ponko_net_messages_steam_host(480, 0, &mut endpoint),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert!(endpoint.is_null());
        for (visibility, max_clients) in [(1, 0), (1, 250), (3, 4)] {
            endpoint = std::ptr::dangling_mut();
            assert_eq!(
                ponko_net_messages_steam_sockets_host(480, visibility, max_clients, &mut endpoint),
                PonkoStatus::PONKO_INVALID_ARGUMENT
            );
            assert!(endpoint.is_null());
        }
        assert_eq!(
            ponko_net_messages_steam_sockets_host(480, 1, 1, std::ptr::null_mut()),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_net_messages_steam_join(480, 0, &mut endpoint),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_steam_lobby_create(480, 3, 4, &mut lobby),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_steam_lobby_create(480, 0, 251, &mut lobby),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_steam_lobby_join(480, 0, &mut lobby),
            PonkoStatus::PONKO_INVALID_ARGUMENT
        );
        assert!(lobby.is_null());
        assert_ne!(ponko_steam_lobby_poll(lobby), PonkoStatus::PONKO_OK);
        assert_ne!(ponko_steam_lobby_leave(lobby), PonkoStatus::PONKO_OK);
        assert_ne!(ponko_steam_lobby_invite(lobby), PonkoStatus::PONKO_OK);
        ponko_steam_lobby_free(lobby);
    }
}
