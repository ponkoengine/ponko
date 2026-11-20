### Added
- Peer-to-peer message endpoints over WebRTC data channels (feature
  `webrtc`): `NetMessages::host_rtc` and `NetMessages::join_rtc`, with
  `RtcConfig` (STUN servers, TURN in browsers, the desktop UDP socket) and
  `Signaling` (any channel carries the setup messages). Browser to browser,
  browser to desktop and desktop to desktop, with the usual reliable and
  unreliable lanes. The desktop side uses str0m on one UDP socket polled
  with the endpoint; the browser side uses `RTCPeerConnection`.
- `LobbyClient::signaling`: the lobby server's relay carries the WebRTC
  signaling between room members.
- `ponko::net::stun`: STUN binding requests and responses (RFC 8489).
- `ponko-lobby` answers STUN requests with `LOBBY_STUN` (rate limited per
  address), and `ponko_lobby::StunResponder` does it in a server of your own.
- C and C++: `ponko_net_messages_rtc_host`, `_rtc_join` and `_rtc_peer`
  (`lobby::rtc_host`, `lobby::rtc_join`, `net_messages::rtc_peer`), with
  `ponko_ffi` built `--features webrtc`.
- The guide's "Peer to peer over WebRTC" chapter, with deployment, the NAT
  cases, the library comparison and what was and wasn't tested.
