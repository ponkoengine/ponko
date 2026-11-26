### Changed
- The experimental features' public items are outside the versions' promise
  now and may change in any release: HDR output (`Game::hdr_output`,
  `ponko::render::Hdr*`, the `Graphics` HDR methods), WebRTC
  (`ponko::net::rtc`, `NetMessages::host_rtc`, `join_rtc`, `rtc_signal_peer`,
  `LobbyClient::signaling`) and the threaded web build (`Game::web_threads`,
  `ponko::web::threads`). They keep their names; `scripts/experimental-api.txt`
  lists them with their reasons, and STABILITY.md, "Unstable", says what that
  means. The cargo features `webrtc` and `hotpatch` are unstable too.
- C: the experimental functions (`ponko_*hdr_output*`, `ponko_ctx_hdr_status`,
  `ponko_net_messages_rtc_*`) carry an `/* Unstable: ... */` note in `ponko.h`
  and in their C++ wrappers' docs, and may change within an ABI version (the
  guide's "C and C++", "Unstable functions").
- The API snapshots list experimental items apart, in
  `api/<crate>/experimental.txt`, and the semver check does not count a break
  of them. `scripts/experimental-api.py check` keeps the list in step with the
  guide's maturity levels, `api/` and the C header.
